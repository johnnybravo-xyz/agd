use urlencoding::decode;

use crate::mcp::OrgModeRouter;

impl OrgModeRouter {
    pub fn decode_uri_path(path: &str) -> String {
        decode(path)
            .map(|cow| cow.into_owned())
            .unwrap_or_else(|_| path.to_string())
    }
}
