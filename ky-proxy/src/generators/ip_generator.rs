use rand::Rng;

pub struct IpGenerator {
    ranges: Vec<IpRange>,
}

struct IpRange {
    start: u32,
    end: u32,
    country: String,
    asn: String,
}

impl IpGenerator {
    pub fn new() -> Self {
        let ranges = vec![
            IpRange {
                start: ip_to_u32("103.21.244.0"),
                end: ip_to_u32("103.21.244.255"),
                country: "India".to_string(),
                asn: "AS13335".to_string(),
            },
            IpRange {
                start: ip_to_u32("103.22.200.0"),
                end: ip_to_u32("103.22.200.255"),
                country: "India".to_string(),
                asn: "AS13335".to_string(),
            },
            IpRange {
                start: ip_to_u32("23.227.38.0"),
                end: ip_to_u32("23.227.38.255"),
                country: "Canada".to_string(),
                asn: "AS13335".to_string(),
            },
        ];
        
        Self { ranges }
    }
    
    pub fn generate_ip(&self) -> (String, String, String) {
        let mut rng = rand::thread_rng();
        let range = &self.ranges[rng.gen_range(0..self.ranges.len())];
        
        let ip_int = rng.gen_range(range.start..=range.end);
        let ip = u32_to_ip(ip_int);
        
        (ip, range.country.clone(), range.asn.clone())
    }
    
    pub fn generate_millions(&self, count: u64) -> Vec<String> {
        (0..count).map(|_| self.generate_ip().0).collect()
    }
}

fn ip_to_u32(ip: &str) -> u32 {
    let parts: Vec<u32> = ip.split('.')
        .map(|p| p.parse().unwrap())
        .collect();
    (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]
}

fn u32_to_ip(ip: u32) -> String {
    format!("{}.{}.{}.{}", 
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    )
}
