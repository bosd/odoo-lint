//! Odoo series versions such as `17.0`, used to gate version-specific rules.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OdooVersion {
    pub major: u16,
    pub minor: u16,
}

impl OdooVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

pub const DEFAULT_ODOO_VERSION: OdooVersion = OdooVersion::new(17, 0);

impl FromStr for OdooVersion {
    type Err = String;

    /// Accepts `17`, `17.0` and `saas~17.2` style strings.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_start_matches("saas~");
        let mut parts = trimmed.split('.');
        let parse = |p: Option<&str>| p.map(|v| v.parse::<u16>());
        match (parse(parts.next()), parse(parts.next()), parts.next()) {
            (Some(Ok(major)), None, None) => Ok(Self::new(major, 0)),
            (Some(Ok(major)), Some(Ok(minor)), None) => Ok(Self::new(major, minor)),
            _ => Err(format!("invalid Odoo version '{s}', expected e.g. \"17.0\"")),
        }
    }
}

impl fmt::Display for OdooVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!("17.0".parse(), Ok(OdooVersion::new(17, 0)));
        assert_eq!("16".parse(), Ok(OdooVersion::new(16, 0)));
        assert_eq!("saas~17.2".parse(), Ok(OdooVersion::new(17, 2)));
        assert!("17.0.1".parse::<OdooVersion>().is_err());
        assert!("latest".parse::<OdooVersion>().is_err());
    }

    #[test]
    fn orders_numerically() {
        assert!(OdooVersion::new(9, 0) < OdooVersion::new(10, 0));
        assert!(OdooVersion::new(17, 0) < OdooVersion::new(17, 2));
    }
}
