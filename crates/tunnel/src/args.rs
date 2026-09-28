//! Options as `--name value` pairs and `--flag`s, each one also from the environment
//! (`PREFIX_NAME`, upper case, `-` as `_`; a flag is on with `1`, `true` or `yes`): a container is
//! configured with environment variables. An argument wins over the environment.

use crate::Error;
use std::collections::HashMap;
use std::str::FromStr;

pub struct Args(HashMap<String, Option<String>>);

/// Where options left out of the arguments are looked up: a prefix and a variable reader.
pub type Env<'a> = (&'a str, &'a dyn Fn(&str) -> Option<String>);

impl Args {
    /// The process's arguments, then its environment under `prefix` (none: arguments only).
    pub fn parse(known: &[&str], prefix: Option<&str>) -> Result<Self, Error> {
        let env = |name: &str| std::env::var(name).ok();
        Self::from(std::env::args().skip(1), known, prefix.map(|p| (p, &env as &dyn Fn(&str) -> _)))
    }

    /// Parses `args`, failing on anything not in `known`; what they leave out comes from `env`.
    pub fn from(
        args: impl Iterator<Item = String>,
        known: &[&str],
        env: Option<Env>,
    ) -> Result<Self, Error> {
        let mut map = HashMap::new();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let name = arg.strip_prefix("--").filter(|n| known.contains(n));
            let name = name.ok_or_else(|| format!("unknown argument {arg}; known: {known:?}"))?;
            let value = args.next_if(|next| !next.starts_with("--"));
            map.insert(name.to_owned(), value);
        }
        if let Some((prefix, lookup)) = env {
            let missing: Vec<_> = known.iter().filter(|n| !map.contains_key(**n)).collect();
            for name in missing {
                let var = format!("{prefix}_{}", name.to_uppercase().replace('-', "_"));
                match lookup(&var).filter(|v| !v.is_empty()) {
                    Some(v) if matches!(v.as_str(), "1" | "true" | "yes") => {
                        map.insert(name.to_string(), None)
                    }
                    Some(v) if matches!(v.as_str(), "0" | "false" | "no") => None,
                    Some(v) => map.insert(name.to_string(), Some(v)),
                    None => None,
                };
            }
        }
        Ok(Self(map))
    }

    pub fn flag(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    pub fn get<T: FromStr>(&self, name: &str) -> Result<Option<T>, Error>
    where
        T::Err: std::fmt::Display,
    {
        let Some(Some(value)) = self.0.get(name) else {
            return Ok(None);
        };
        value.parse().map(Some).map_err(|e| format!("--{name} {value}: {e}").into())
    }

    pub fn required<T: FromStr>(&self, name: &str) -> Result<T, Error>
    where
        T::Err: std::fmt::Display,
    {
        self.get(name)?
            .ok_or_else(|| format!("--{name} (or its environment variable) is required").into())
    }
}

#[cfg(test)]
mod tests {
    use super::Args;

    #[test]
    fn arguments_win_over_the_environment_and_flags_take_words() {
        let env = |name: &str| match name {
            "GW_LISTEN" => Some("0.0.0.0:443".to_owned()),
            "GW_TARGET" => Some("from-env".to_owned()),
            "GW_NO_CLIENT_AUTH" => Some("true".to_owned()),
            "GW_CONGESTION" => Some("".to_owned()),
            _ => None,
        };
        let args = ["--target", "from-args"].map(String::from).into_iter();
        let known = ["listen", "target", "no-client-auth", "congestion"];
        let parsed = Args::from(args, &known, Some(("GW", &env))).unwrap();
        assert_eq!(parsed.get::<String>("target").unwrap().as_deref(), Some("from-args"));
        assert_eq!(parsed.get::<String>("listen").unwrap().as_deref(), Some("0.0.0.0:443"));
        assert!(parsed.flag("no-client-auth"));
        assert!(!parsed.flag("congestion"), "an empty variable is not set");
    }
}
