//! Host identity, not browser identity or a claim of native API availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Macos,
    Linux,
    Windows,
    Unsupported,
}
impl Os {
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            "macos" => Self::Macos,
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            _ => Self::Unsupported,
        }
    }
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Unsupported => "unsupported",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Platform {
    pub os: Os,
    pub architecture: &'static str,
}
pub(crate) trait Driver {
    fn identify(&self) -> Platform;
}
struct Native;
impl Driver for Native {
    fn identify(&self) -> Platform {
        Platform {
            os: Os::from_name(std::env::consts::OS),
            architecture: std::env::consts::ARCH,
        }
    }
}
pub(crate) fn current() -> Platform {
    Native.identify()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(Platform);
    impl Driver for Fixture {
        fn identify(&self) -> Platform {
            self.0
        }
    }
    #[test]
    fn typed_identity_is_injectable_and_unknown_hosts_are_not_guessed() {
        for (name, os) in [
            ("macos", Os::Macos),
            ("linux", Os::Linux),
            ("windows", Os::Windows),
            ("other", Os::Unsupported),
        ] {
            let driver: &dyn Driver = &Fixture(Platform {
                os: Os::from_name(name),
                architecture: "fixture",
            });
            assert_eq!(driver.identify().os, os);
            assert_eq!(driver.identify().architecture, "fixture");
            assert_eq!(Os::from_name(os.name()), os);
        }
        assert_eq!(current().architecture, std::env::consts::ARCH);
        assert_eq!(current().os, Os::from_name(std::env::consts::OS));
    }
}
