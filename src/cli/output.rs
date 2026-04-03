use crate::execution::result::RunOutput;

/// Print a RunOutput to stdout.
pub fn print_run_output(output: &RunOutput) {
    if !output.text.is_empty() {
        println!("{}", output.text);
    }
}
