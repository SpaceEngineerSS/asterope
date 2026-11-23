use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;

/// Command-line arguments split into positionals, `--name value` options
/// and `--name` switches.
pub struct Args {
    positional: Vec<String>,
    values: HashMap<String, String>,
    switches: Vec<String>,
}

impl Args {
    /// `options` take a value, `switches` do not; anything else starting
    /// with `--` is an error.
    pub fn parse(
        args: &[String],
        options: &[&str],
        switches: &[&str],
    ) -> Result<Args, Box<dyn Error>> {
        let mut parsed = Args {
            positional: Vec::new(),
            values: HashMap::new(),
            switches: Vec::new(),
        };
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            match arg.strip_prefix("--") {
                Some(name) if options.contains(&name) => {
                    let value = rest.next().ok_or(format!("missing value for --{name}"))?;
                    parsed.values.insert(name.to_string(), value.clone());
                }
                Some(name) if switches.contains(&name) => parsed.switches.push(name.to_string()),
                Some(name) => return Err(format!("unknown option --{name}").into()),
                None => parsed.positional.push(arg.clone()),
            }
        }
        Ok(parsed)
    }

    pub fn positional(&self) -> &[String] {
        &self.positional
    }

    pub fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn value<T>(&self, name: &str) -> Result<Option<T>, Box<dyn Error>>
    where
        T: FromStr,
        T::Err: Error + 'static,
    {
        match self.values.get(name) {
            Some(text) => text
                .parse()
                .map(Some)
                .map_err(|e: T::Err| format!("--{name} {text}: {e}").into()),
            None => Ok(None),
        }
    }

    pub fn value_or<T>(&self, name: &str, default: T) -> Result<T, Box<dyn Error>>
    where
        T: FromStr,
        T::Err: Error + 'static,
    {
        Ok(self.value(name)?.unwrap_or(default))
    }
}
