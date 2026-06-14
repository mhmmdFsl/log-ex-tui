use std::sync::Arc;

use tonic::service::Interceptor;
use tonic::{Request, Status};

use super::token::TokenCache;

pub struct AuthInterceptor {
    cache: Arc<TokenCache>,
}

impl AuthInterceptor {
    pub fn new(cache: Arc<TokenCache>) -> Self {
        Self { cache }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = self
            .cache
            .get_sync()
            .map_err(|e| Status::unauthenticated(format!("auth: {e}")))?;

        let value = format!("Bearer {token}")
            .parse()
            .map_err(|_| Status::internal("invalid auth header value"))?;

        request.metadata_mut().insert("authorization", value);
        Ok(request)
    }
}
