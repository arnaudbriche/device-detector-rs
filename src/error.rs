#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    YAML(#[from] serde_yaml::Error),
    #[error(transparent)]
    Regex(#[from] fancy_regex::Error),
    #[error(transparent)]
    RegexFilteredParse(#[from] regex_filtered::ParseError),
    #[error(transparent)]
    RegexFilteredBuild(#[from] regex_filtered::BuildError),
}

pub type Result<T> = std::result::Result<T, Error>;
