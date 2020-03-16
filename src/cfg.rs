use serde::Deserialize;
#[derive(Deserialize, Debug)]
pub(crate) struct Config {
    pub(crate) programs: Vec<String>,
    pub(crate) rounds: u32,
    pub(crate) image: Option<String>,
}
