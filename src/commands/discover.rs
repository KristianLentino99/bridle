use crate::adapters;
use crate::platform;

pub fn run() {
    let plat = platform::detect();
    println!("Platform: {}", plat.name());
    println!();

    let statuses = adapters::detect_all(plat);
    for status in &statuses {
        let icon = if status.installed { "✅" } else { "❌" };
        let mcp_icon = if status.mcp_config_exists {
            "📄"
        } else {
            "  "
        };
        println!(
            "{} {} {} ({})",
            icon, mcp_icon, status.spec.name, status.spec.id
        );
    }
}
