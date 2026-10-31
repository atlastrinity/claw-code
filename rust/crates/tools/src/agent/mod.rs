pub(crate) mod execute;
pub(crate) mod provider;
pub(crate) mod summary;

#[allow(unused_imports)]
pub(crate) use execute::*;
#[allow(unused_imports)]
pub(crate) use provider::*;
#[allow(unused_imports)]
pub(crate) use summary::*;

use crate::tool_types::*;
use crate::util::to_pretty_json;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_agent(input: AgentInput) -> Result<String, String> {
    let result = execute::execute_agent(input)?;
    to_pretty_json(result)
}
