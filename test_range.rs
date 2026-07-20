use cron::Schedule; use std::str::FromStr; fn main() { println!("{:?}", Schedule::from_str("0 0 18-2 * * *")); }
