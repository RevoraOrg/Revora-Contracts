#![deny(clippy::arithmetic_side_effects)]

use crate::RevoraError;

pub trait SafeMath: Sized {
    fn s_add(self, other: Self) -> Result<Self, RevoraError>;
    fn s_sub(self, other: Self) -> Result<Self, RevoraError>;
    fn s_mul(self, other: Self) -> Result<Self, RevoraError>;
    fn s_div(self, other: Self) -> Result<Self, RevoraError>;
}

macro_rules! impl_safe_math {
    ($($t:ty),+) => {
        $(
            impl SafeMath for $t {
                fn s_add(self, other: Self) -> Result<Self, RevoraError> {
                    self.checked_add(other).ok_or(RevoraError::LimitReached)
                }

                fn s_sub(self, other: Self) -> Result<Self, RevoraError> {
                    self.checked_sub(other).ok_or(RevoraError::LimitReached)
                }

                fn s_mul(self, other: Self) -> Result<Self, RevoraError> {
                    self.checked_mul(other).ok_or(RevoraError::LimitReached)
                }

                fn s_div(self, other: Self) -> Result<Self, RevoraError> {
                    self.checked_div(other).ok_or(RevoraError::LimitReached)
                }
            }
        )+
    };
}

impl_safe_math!(u32, u64, i128);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RevoraError;

    #[test]
    fn test_s_add() {
        assert_eq!(10_u32.s_add(20_u32), Ok(30_u32));
        assert_eq!(u32::MAX.s_add(1), Err(RevoraError::LimitReached));
    }

    #[test]
    fn test_s_sub() {
        assert_eq!(20_u32.s_sub(10_u32), Ok(10_u32));
        assert_eq!(0_u32.s_sub(1), Err(RevoraError::LimitReached));
    }

    #[test]
    fn test_s_mul() {
        assert_eq!(10_u32.s_mul(20_u32), Ok(200_u32));
        assert_eq!(u32::MAX.s_mul(2), Err(RevoraError::LimitReached));
    }

    #[test]
    fn test_s_div() {
        assert_eq!(20_u32.s_div(10_u32), Ok(2_u32));
        assert_eq!(20_u32.s_div(0), Err(RevoraError::LimitReached));
    }
}
