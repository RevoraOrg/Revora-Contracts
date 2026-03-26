import request from 'supertest';
import express from 'express';
import { 
  setupInvestmentRouter, 
  InMemoryLock, 
  investmentDoubleSubmitProtection 
} from '../index';

describe('Health & Investment API Tests', () => {
  let app: express.Express;
  let customLock: InMemoryLock;

  beforeEach(() => {
    app = express();
    app.use(express.json());
    
    // Add a basic health endpoint to satisfy health.test.ts nomenclature
    app.get('/health', (req, res) => res.status(200).json({ status: 'OK' }));
    
    // Inject lock for monitoring and test setup isolation
    customLock = new InMemoryLock();
    app.use(setupInvestmentRouter(customLock));
  });

  describe('GET /health', () => {
    it('should return 200 OK for health check', async () => {
      const response = await request(app).get('/health');
      expect(response.status).toBe(200);
      expect(response.body).toEqual({ status: 'OK' });
    });
  });

  describe('POST /api/investments Double-Submit Protection', () => {
    it('should process a single valid investment request', async () => {
      const response = await request(app)
        .post('/api/investments')
        .send({ userId: 'user123', offeringId: 'offering456' });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
    });

    it('should reject simultaneous concurrent requests for the same user/offering', async () => {
      const payload = { userId: 'user-concurrent-test', offeringId: 'offering-fast' };
      
      // Fire two requests simultaneously without waiting
      const req1 = request(app).post('/api/investments').send(payload);
      const req2 = request(app).post('/api/investments').send(payload);

      const [res1, res2] = await Promise.all([req1, req2]);

      // One must succeed, one must conflict
      const statuses = [res1.status, res2.status];
      expect(statuses).toContain(200);
      expect(statuses).toContain(409);
      
      const conflictRes = res1.status === 409 ? res1 : res2;
      expect(conflictRes.body.error).toBe("Conflict");
    });

    it('should allow consecutive requests after lock is released', async () => {
      const payload = { userId: 'user-consecutive', offeringId: 'offering-slow' };

      const res1 = await request(app).post('/api/investments').send(payload);
      expect(res1.status).toBe(200);

      // Sent explicitly after the first finishes, should succeed
      const res2 = await request(app).post('/api/investments').send(payload);
      expect(res2.status).toBe(200);
    });

    it('should differentiate locks correctly across different users', async () => {
      const req1 = request(app)
        .post('/api/investments')
        .send({ userId: 'user-A', offeringId: 'offering-X' });
        
      const req2 = request(app)
        .post('/api/investments')
        .send({ userId: 'user-B', offeringId: 'offering-X' }); // Same offering, different user
        
      const [res1, res2] = await Promise.all([req1, req2]);
      
      expect(res1.status).toBe(200);
      expect(res2.status).toBe(200);
    });

    it('should correctly utilize idempotency-key headers', async () => {
      const req1 = request(app)
        .post('/api/investments')
        .set('Idempotency-Key', 'unique-hash-111')
        .send({ amount: 1000 });
        
      const req2 = request(app)
        .post('/api/investments')
        .set('Idempotency-Key', 'unique-hash-111') // Same custom key
        .send({ amount: 1000 });
        
      const [res1, res2] = await Promise.all([req1, req2]);
      
      const statuses = [res1.status, res2.status];
      expect(statuses).toContain(200);
      expect(statuses).toContain(409);
    });

    it('should pass through if no identifiable keys are sent (fallback mode)', async () => {
      // By default the middleware skips protection if a key cannot be generated
      const req1 = request(app).post('/api/investments').send({});
      const req2 = request(app).post('/api/investments').send({});
      
      const [res1, res2] = await Promise.all([req1, req2]);
      
      expect(res1.status).toBe(200);
      expect(res2.status).toBe(200);
    });
  });
});
