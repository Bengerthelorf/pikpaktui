use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    super::run_transfer(args, "mv", "move", "Moved", |client, ids, dst| {
        client.mv(ids, dst)
    })
}
