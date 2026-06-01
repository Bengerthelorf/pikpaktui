use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    super::run_star_toggle(args, "star", "Starred", |client, ids| client.star(ids))
}
