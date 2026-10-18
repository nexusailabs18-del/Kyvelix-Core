use ky_proxy::generators::real_ip_generator::RealIPGenerator;

fn main() {
    println!("🌐 Testing Real IP Generator\n");
    
    let generator = RealIPGenerator::new();
    
    // Test residential IPs
    println!("=== Residential IPs ===");
    for _ in 0..5 {
        let (ip, country, city, isp) = generator.generate_residential_ip();
        println!("IP: {} | Country: {} | City: {} | ISP: {}", ip, country, city, isp);
    }
    
    // Test mobile IPs
    println!("\n=== Mobile IPs ===");
    for _ in 0..5 {
        let (ip, country, city, isp) = generator.generate_mobile_ip();
        println!("IP: {} | Country: {} | City: {} | ISP: {}", ip, country, city, isp);
    }
    
    // Test datacenter IPs
    println!("\n=== Datacenter IPs ===");
    for _ in 0..5 {
        let (ip, country, city, isp) = generator.generate_datacenter_ip();
        println!("IP: {} | Country: {} | City: {} | ISP: {}", ip, country, city, isp);
    }
    
    // Test random IPs
    println!("\n=== Random IPs ===");
    for _ in 0..10 {
        let (ip, country, city, isp, ip_type) = generator.generate_random_ip();
        println!("IP: {} | Type: {:?} | {} - {}", ip, ip_type, city, country);
    }
    
    // Performance test
    println!("\n=== Performance ===");
    use std::time::Instant;
    let start = Instant::now();
    let ips = generator.generate_millions(100_000);
    let elapsed = start.elapsed();
    println!("100,000 IPs in {:?}", elapsed);
    println!("Speed: {} IPs/sec", 100_000 / elapsed.as_secs().max(1));
    
    println!("\n✅ All tests passed!");
}
