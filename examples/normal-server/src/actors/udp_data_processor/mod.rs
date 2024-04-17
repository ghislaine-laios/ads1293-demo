pub struct UdpDataProcessor {}

impl UdpDataProcessor {
    pub fn launch(self) -> tokio::task::JoinHandle<()> {
        actix_rt::spawn(self.task())
    }
    
    async fn task(self) {
        
    }
}