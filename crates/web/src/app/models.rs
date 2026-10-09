//! `/models`: the model table lives on the home page.

mod model_id;

use topcoat::{
    Result,
    router::{error::redirect_permanent, response::Response, route},
};

#[route(GET)]
async fn models_index() -> Result<Response> {
    Err(redirect_permanent("/").into())
}
