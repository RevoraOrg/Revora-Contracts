import { Request, Response, NextFunction, Router } from 'express';

/**
 * Interface for a generic distributed lock mechanism.
 */
export interface DistributedLock {
  acquire(key: string, ttlMs: number): Promise<boolean>;
  release(key: string): Promise<void>;
}

/**
 * An in-memory lock implementation for single-instance, lightweight deployments.
 * Production environments should inject a Redis-based DistributedLock.
 */
export class InMemoryLock implements DistributedLock {
  private locks: Map<string, number> = new Map();

  async acquire(key: string, ttlMs: number): Promise<boolean> {
    const now = Date.now();
    const expiresAt = this.locks.get(key);
    
    if (expiresAt && expiresAt > now) {
      return false; // Lock already held
    }
    
    this.locks.set(key, now + ttlMs);
    return true;
  }

  async release(key: string): Promise<void> {
    this.locks.delete(key);
  }
}

/**
 * Default global in-memory lock instance for the middleware.
 */
export const defaultLock = new InMemoryLock();

/**
 * Options for the double submit protection middleware.
 */
export interface DoubleSubmitProtectionOptions {
  /** The lock implementation to use. Defaults to `defaultLock` (in-memory). */
  lock?: DistributedLock;
  /** TTL for the lock in milliseconds. Defaults to 10 seconds. */
  ttlMs?: number;
  /** 
   * A function to generate a unique lock key from the request. 
   * The default generates a key like `invest:lock:<userId>:<offeringId>`, 
   * assuming they are provided in `req.body`.
   */
  keyGenerator?: (req: Request) => string | null;
}

/**
 * Middleware to protect against duplicate simultaneous investment submissions.
 * It attempts to acquire a lock for the specific transaction vector.
 * If the lock cannot be acquired, a 409 Conflict is returned.
 * The lock is released automatically after the request finishes.
 */
export const investmentDoubleSubmitProtection = (options?: DoubleSubmitProtectionOptions) => {
  const lock = options?.lock || defaultLock;
  const ttlMs = options?.ttlMs || 10000;
  
  const defaultKeyGen = (req: Request): string | null => {
    // Assuming standard layout: req.body.userId and req.body.offeringId
    const userId = req.body?.userId || req.headers['x-user-id'];
    const offeringId = req.body?.offeringId || req.params?.offeringId;
    
    // As a fallback, use an idempotency key header if provided
    const idempotencyKey = req.headers['idempotency-key'] || req.headers['x-idempotency-key'];

    if (idempotencyKey) {
      return `invest:lock:idem:${idempotencyKey}`;
    }

    if (userId && offeringId) {
      return `invest:lock:${userId}:${offeringId}`;
    }

    return null; // Cannot determine key, skip protection (or could throw)
  };

  const keyGenerator = options?.keyGenerator || defaultKeyGen;

  return async (req: Request, res: Response, next: NextFunction): Promise<void> => {
    const lockKey = keyGenerator(req);
    
    if (!lockKey) {
      // Note: for strict production, pass to next() to allow requests that don't match the signature.
      // Alternatively, you could throw a 400 Bad Request here for strict enforcement.
      next();
      return;
    }

    const acquired = await lock.acquire(lockKey, ttlMs);
    if (!acquired) {
      res.status(409).json({
        error: "Conflict",
        message: "An investment request for this offering is already processing. Please wait."
      });
      return;
    }

    // Attach release hook to response finish/close event to guarantee release
    const releaseLock = async () => {
      try {
        await lock.release(lockKey);
      } catch (e) {
        console.error("Failed to release lock:", e);
      }
    };

    res.on('finish', releaseLock);
    res.on('close', releaseLock);

    next();
  };
};

/**
 * Reference router setup demonstrating how the middleware integrates.
 */
export const setupInvestmentRouter = (lockWrapper?: DistributedLock) => {
  const router = Router();
  
  router.post(
    '/api/investments',
    investmentDoubleSubmitProtection({ lock: lockWrapper }),
    (req, res) => {
      // Simulate slow processing (e.g., blockchain tx submission or complex verification)
      setTimeout(() => {
        res.status(200).json({ success: true, message: "Investment processed." });
      }, 50); // 50ms delay
    }
  );

  return router;
};
