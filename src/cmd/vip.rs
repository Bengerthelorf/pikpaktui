use anyhow::Result;

pub fn run() -> Result<()> {
    let client = super::cli_client()?;
    let resp = client.vip_info()?;

    if let Some(data) = resp.data {
        let vip_type = data.vip_type.as_deref().unwrap_or("none");
        let status = data.status.as_deref().unwrap_or("unknown");
        let expire = data.expire.as_deref().unwrap_or("N/A");

        println!("VIP Type:   {}", vip_type);
        println!("Status:     {}", status);
        println!("Expires:    {}", expire);
    } else {
        println!("No VIP info available");
    }

    // Invite code
    match client.invite_code() {
        Ok(code) => println!("Invite Code: {}", code),
        Err(_) => {}
    }

    // Transfer quota
    match client.transfer_quota() {
        Ok(val) => {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    let kind = item["kind"].as_str().unwrap_or("?");
                    let total = item["total"].as_i64().unwrap_or(0);
                    let used = item["used"].as_i64().unwrap_or(0);
                    println!("Transfer {}: {}/{}", kind, used, total);
                }
            }
        }
        Err(_) => {}
    }

    Ok(())
}
