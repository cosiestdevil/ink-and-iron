use clap::ValueEnum;
pub use llm::*;
#[derive(Clone, Copy, ValueEnum, Debug)]
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
