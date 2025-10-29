use worker::*;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/stream/:track", handle_stream)
        .run(req, env)
        .await
}

async fn handle_stream(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    // TODO: Implement streaming from R2 with rate limiting
    Response::error("Not implemented", 501)
}
