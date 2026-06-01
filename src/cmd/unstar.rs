use anyhow::Result;

pub fn run(args: &[String]) -> Result<()> {
    super::run_star_toggle(args, "unstar", "Unstarred", |client, ids| {
        client.unstar(ids)
    })
}
