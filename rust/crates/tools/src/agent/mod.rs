pub(crate) mod execute;
pub(crate) mod summary;
pub(crate) mod provider;

pub(crate) use execute::*;
pub(crate) use summary::*;
pub(crate) use provider::*;


use crate::tool_types::*;
use crate::util::to_pretty_json;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_agent(input: AgentInput) -> Result<String, String> {
    let result = execute::execute_agent(input)?;
    to_pretty_json(result)
}
