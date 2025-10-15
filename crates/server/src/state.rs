#[derive(Clone)]
pub struct AppState {}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        Ok(Self{})
    }
}
