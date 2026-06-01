use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    super::run_transfer(args, "cp", "copy", "Copied", |client, ids, dst| {
        client.cp(ids, dst)
    })
}
