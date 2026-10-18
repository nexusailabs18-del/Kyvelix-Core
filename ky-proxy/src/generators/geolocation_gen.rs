use rand::Rng;

pub struct GeolocationGenerator {
    locations: Vec<Location>,
}

struct Location {
    country: String,
    city: String,
    lat: f64,
    lon: f64,
    timezone: String,
    zip: String,
}

impl GeolocationGenerator {
    pub fn new() -> Self {
        Self {
            locations: vec![
                Location { country: "US".to_string(), city: "New York".to_string(), lat: 40.7128, lon: -74.0060, timezone: "America/New_York".to_string(), zip: "10001".to_string() },
                Location { country: "UK".to_string(), city: "London".to_string(), lat: 51.5074, lon: -0.1278, timezone: "Europe/London".to_string(), zip: "SW1A".to_string() },
                Location { country: "IN".to_string(), city: "Mumbai".to_string(), lat: 19.0760, lon: 72.8777, timezone: "Asia/Kolkata".to_string(), zip: "400001".to_string() },
            ],
        }
    }
    
    pub fn generate_location(&self) -> (String, String, f64, f64, String, String) {
        let mut rng = rand::thread_rng();
        let loc = &self.locations[rng.gen_range(0..self.locations.len())];
        (loc.country.clone(), loc.city.clone(), loc.lat, loc.lon, loc.timezone.clone(), loc.zip.clone())
    }
}
