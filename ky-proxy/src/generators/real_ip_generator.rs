use rand::Rng;
use std::net::Ipv4Addr;

pub struct RealIPGenerator {
    residential_ranges: Vec<IPRange>,
    mobile_ranges: Vec<IPRange>,
    datacenter_ranges: Vec<IPRange>,
    isp_ranges: Vec<IPRange>,
}

#[derive(Debug, Clone)]
pub struct IPRange {
    start: u32,
    end: u32,
    country: String,
    city: String,
    asn: String,
    isp: String,
    ip_type: IPType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IPType {
    Residential,
    Mobile,
    Datacenter,
    ISP,
}

impl RealIPGenerator {
    pub fn new() -> Self {
        Self {
            residential_ranges: Self::load_residential_ranges(),
            mobile_ranges: Self::load_mobile_ranges(),
            datacenter_ranges: Self::load_datacenter_ranges(),
            isp_ranges: Self::load_isp_ranges(),
        }
    }
    
    fn load_residential_ranges() -> Vec<IPRange> {
        vec![
            // US Residential
            IPRange {
                start: ip_to_u32("24.0.0.0"),
                end: ip_to_u32("24.255.255.255"),
                country: "US".to_string(),
                city: "New York".to_string(),
                asn: "AS7922".to_string(),
                isp: "Comcast".to_string(),
                ip_type: IPType::Residential,
            },
            IPRange {
                start: ip_to_u32("71.0.0.0"),
                end: ip_to_u32("71.255.255.255"),
                country: "US".to_string(),
                city: "Los Angeles".to_string(),
                asn: "AS7018".to_string(),
                isp: "AT&T".to_string(),
                ip_type: IPType::Residential,
            },
            // UK Residential
            IPRange {
                start: ip_to_u32("81.0.0.0"),
                end: ip_to_u32("81.255.255.255"),
                country: "UK".to_string(),
                city: "London".to_string(),
                asn: "AS2856".to_string(),
                isp: "BT".to_string(),
                ip_type: IPType::Residential,
            },
        ]
    }
    
    fn load_mobile_ranges() -> Vec<IPRange> {
        vec![
            // US Mobile (Verizon)
            IPRange {
                start: ip_to_u32("174.192.0.0"),
                end: ip_to_u32("174.255.255.255"),
                country: "US".to_string(),
                city: "Mobile".to_string(),
                asn: "AS22394".to_string(),
                isp: "Verizon Wireless".to_string(),
                ip_type: IPType::Mobile,
            },
            // India Mobile (Jio)
            IPRange {
                start: ip_to_u32("49.32.0.0"),
                end: ip_to_u32("49.47.255.255"),
                country: "IN".to_string(),
                city: "Mumbai".to_string(),
                asn: "AS55836".to_string(),
                isp: "Reliance Jio".to_string(),
                ip_type: IPType::Mobile,
            },
        ]
    }
    
    fn load_datacenter_ranges() -> Vec<IPRange> {
        vec![
            // AWS
            IPRange {
                start: ip_to_u32("3.0.0.0"),
                end: ip_to_u32("3.255.255.255"),
                country: "US".to_string(),
                city: "Ashburn".to_string(),
                asn: "AS14618".to_string(),
                isp: "AWS".to_string(),
                ip_type: IPType::Datacenter,
            },
            // Google Cloud
            IPRange {
                start: ip_to_u32("34.0.0.0"),
                end: ip_to_u32("34.255.255.255"),
                country: "US".to_string(),
                city: "Mountain View".to_string(),
                asn: "AS15169".to_string(),
                isp: "Google Cloud".to_string(),
                ip_type: IPType::Datacenter,
            },
        ]
    }
    
    fn load_isp_ranges() -> Vec<IPRange> {
        vec![
            // Your real ISP
            IPRange {
                start: ip_to_u32("171.76.0.0"),
                end: ip_to_u32("171.79.255.255"),
                country: "IN".to_string(),
                city: "Bangalore".to_string(),
                asn: "AS24560".to_string(),
                isp: "Airtel".to_string(),
                ip_type: IPType::ISP,
            },
        ]
    }
    
    pub fn generate_residential_ip(&self) -> (String, String, String, String) {
        let range = &self.residential_ranges[rand::thread_rng().gen_range(0..self.residential_ranges.len())];
        let ip = generate_ip_in_range(range);
        (ip, range.country.clone(), range.city.clone(), range.isp.clone())
    }
    
    pub fn generate_mobile_ip(&self) -> (String, String, String, String) {
        let range = &self.mobile_ranges[rand::thread_rng().gen_range(0..self.mobile_ranges.len())];
        let ip = generate_ip_in_range(range);
        (ip, range.country.clone(), range.city.clone(), range.isp.clone())
    }
    
    pub fn generate_datacenter_ip(&self) -> (String, String, String, String) {
        let range = &self.datacenter_ranges[rand::thread_rng().gen_range(0..self.datacenter_ranges.len())];
        let ip = generate_ip_in_range(range);
        (ip, range.country.clone(), range.city.clone(), range.isp.clone())
    }
    
    pub fn generate_random_ip(&self) -> (String, String, String, String, IPType) {
        let mut rng = rand::thread_rng();
        let all_ranges = [
            &self.residential_ranges,
            &self.mobile_ranges,
            &self.datacenter_ranges,
            &self.isp_ranges,
        ];
        
        let range_set = all_ranges[rng.gen_range(0..all_ranges.len())];
        let range = &range_set[rng.gen_range(0..range_set.len())];
        let ip = generate_ip_in_range(range);
        
        (ip, range.country.clone(), range.city.clone(), range.isp.clone(), range.ip_type.clone())
    }
    
    pub fn generate_millions(&self, count: u64) -> Vec<(String, String, String, String)> {
        (0..count).map(|_| {
            let (ip, country, city, isp) = self.generate_residential_ip();
            (ip, country, city, isp)
        }).collect()
    }
}

fn ip_to_u32(ip: &str) -> u32 {
    let parts: Vec<u32> = ip.split('.').map(|p| p.parse().unwrap()).collect();
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

fn generate_ip_in_range(range: &IPRange) -> String {
    let mut rng = rand::thread_rng();
    let ip_int = rng.gen_range(range.start..=range.end);
    u32_to_ip(ip_int)
}
