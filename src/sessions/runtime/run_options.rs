/// Options for a single run. Passed to `run_with_options()`.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub system_overlay: Option<String>,
    pub skill_overlay: Option<String>,
    pub max_iterations: Option<u32>,
    pub max_duration_secs: Option<u64>,
}
