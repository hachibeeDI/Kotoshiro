use anyhow::Result;
use clap::Args as ClapArgs;

use super::Ctx;
use crate::store::lock;
use crate::util::parse_issue;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    #[arg(long, value_name = "org/repo#N")]
    pub issue: String,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let issue = parse_issue(&a.issue)?;
    let _g = lock::lock_todo(&ctx.store, a.id)?;
    let mut t = ctx.load(a.id)?;
    t.meta.issue = Some(issue.clone());
    t.touch(ctx.now);
    ctx.save(&t)?;
    if ctx.json {
        ctx.print_json(&t.meta)
    } else {
        println!("#{} linked to {issue}", a.id);
        Ok(())
    }
}
