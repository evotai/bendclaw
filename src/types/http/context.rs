#[derive(Clone, Debug, Default)]
pub struct HttpRequestContext {
    pub service: String,
    pub operation: String,
    pub endpoint: String,
    pub model: Option<String>,
    pub warehouse: Option<String>,
    pub url: String,
}

impl HttpRequestContext {
    pub fn new(service: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            operation: operation.into(),
            endpoint: String::new(),
            model: None,
            warehouse: None,
            url: String::new(),
        }
    }

    pub fn with_endpoint(mut self, value: impl Into<String>) -> Self {
        self.endpoint = value.into();
        self
    }

    pub fn with_model(mut self, value: impl Into<String>) -> Self {
        self.model = Some(value.into());
        self
    }

    pub fn with_warehouse(mut self, value: impl Into<String>) -> Self {
        self.warehouse = Some(value.into());
        self
    }

    pub fn with_url(mut self, value: impl Into<String>) -> Self {
        self.url = value.into();
        self
    }
}
