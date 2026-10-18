use rand::Rng;

pub struct BehaviorGenerator {
    mouse_movements: Vec<(f64, f64)>,
    scroll_patterns: Vec<String>,
    typing_speeds: Vec<f64>,
}

impl BehaviorGenerator {
    pub fn new() -> Self {
        Self {
            mouse_movements: vec![(10.0, 20.0), (50.0, 100.0), (200.0, 300.0)],
            scroll_patterns: vec!["slow".to_string(), "medium".to_string(), "fast".to_string(), "random".to_string()],
            typing_speeds: vec![50.0, 75.0, 100.0],
        }
    }
    
    pub fn generate_behavior(&self) -> BehaviorPattern {
        let mut rng = rand::thread_rng();
        BehaviorPattern {
            mouse_speed: rng.gen_range(50..200),
            scroll_speed: rng.gen_range(100..500),
            typing_speed: self.typing_speeds[rng.gen_range(0..self.typing_speeds.len())],
            dwell_time: rng.gen_range(2..30),
            click_probability: rng.gen_range(0.1..0.9),
        }
    }
}

pub struct BehaviorPattern {
    pub mouse_speed: i32,
    pub scroll_speed: i32,
    pub typing_speed: f64,
    pub dwell_time: i32,
    pub click_probability: f64,
}
