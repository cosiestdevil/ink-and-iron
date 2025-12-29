use clap::ValueEnum;
pub use llm::*;
#[derive(Clone, Copy, ValueEnum, Debug, serde::Serialize, serde::Deserialize)]
pub enum LLMMode {
    None = 0,
    Cpu = 1,
    Cuda = 2,
}
impl std::fmt::Display for LLMMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMMode::None => write!(f, "none"),
            LLMMode::Cpu => write!(f, "cpu"),
            LLMMode::Cuda => write!(f, "cuda"),
        }
    }
}
impl From<LLMMode> for llm::LLMMode {
    fn from(value: LLMMode) -> Self {
        match value {
            LLMMode::None => llm::LLMMode::None,
            LLMMode::Cpu => llm::LLMMode::Cpu,
            LLMMode::Cuda => llm::LLMMode::Cuda,
        }
    }
}
impl From<LLMMode> for menu::LLMMode {
    fn from(value: LLMMode) -> Self {
        match value {
            LLMMode::None => menu::LLMMode::None,
            LLMMode::Cpu => menu::LLMMode::Cpu,
            LLMMode::Cuda => menu::LLMMode::Cuda,
        }
    }
}
impl From<menu::LLMMode> for LLMMode {
    fn from(value: menu::LLMMode) -> Self {
        match value {
            menu::LLMMode::None => LLMMode::None,
            menu::LLMMode::Cpu => LLMMode::Cpu,
            menu::LLMMode::Cuda => LLMMode::Cuda,
        }
    }
}
