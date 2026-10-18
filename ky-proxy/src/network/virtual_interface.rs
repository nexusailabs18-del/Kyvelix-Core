use std::process::Command;
use tracing::info;

pub struct VirtualInterface {
    name: String,
    ip: String,
    is_up: bool,
}

impl VirtualInterface {
    pub fn new(name: &str, ip: &str) -> Self {
        Self {
            name: name.to_string(),
            ip: ip.to_string(),
            is_up: false,
        }
    }
    
    pub fn create(&mut self) -> anyhow::Result<()> {
        // Create dummy interface
        Command::new("sudo")
            .args(["ip", "link", "add", &self.name, "type", "dummy"])
            .output()?;
        
        // Assign IP
        Command::new("sudo")
            .args(["ip", "addr", "add", &format!("{}/32", self.ip), "dev", &self.name])
            .output()?;
        
        // Bring up
        Command::new("sudo")
            .args(["ip", "link", "set", &self.name, "up"])
            .output()?;
        
        self.is_up = true;
        info!("✅ Virtual interface {} created with IP {}", self.name, self.ip);
        Ok(())
    }
    
    pub fn destroy(&mut self) -> anyhow::Result<()> {
        if self.is_up {
            Command::new("sudo")
                .args(["ip", "link", "delete", &self.name])
                .output()?;
            self.is_up = false;
        }
        Ok(())
    }
    
    pub fn route_through(&self, target: &str) -> anyhow::Result<()> {
        // Route traffic through this interface
        Command::new("sudo")
            .args(["ip", "route", "add", target, "dev", &self.name])
            .output()?;
        info!("✅ Traffic to {} routed through {}", target, self.name);
        Ok(())
    }
}
