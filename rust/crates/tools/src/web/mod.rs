pub(crate) mod fetch;
pub(crate) mod search;

pub(crate) use fetch::*;

use crate::tool_types::*;
use crate::util::to_pretty_json;

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_web_fetch(input: WebFetchInput) -> Result<String, String> {
    let result = fetch::execute_web_fetch(&input)?;
    to_pretty_json(result)
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_web_search(input: WebSearchInput) -> Result<String, String> {
    let result = fetch::execute_web_search(&input)?;
    to_pretty_json(result)
}
