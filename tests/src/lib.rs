#[cfg(test)]
mod test{

    use std::{env, path::PathBuf, path::Path};
    use std::process::Command;

    const CUR_DIR: &str = "tests";
    const TSH: &str = "target/debug/tsh";
    const RUNENV: &str = "bin/";

    fn get_workspace_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()

    }

    fn get_tsh() -> PathBuf {
        let mut cwd = get_workspace_dir();
        cwd.push(TSH);
        cwd
    }

    #[test]
    fn run(){
        env::set_current_dir(format!("{}/{}",get_workspace_dir().to_str().unwrap(),RUNENV)).unwrap();
        Command::new("runtest")
            .arg("./sdriver.pl")
            .arg(format!("{}/{}/",get_workspace_dir().to_str().unwrap(),CUR_DIR))
            .arg(get_tsh().to_str().unwrap())
            .status()
            .unwrap();
    }
}
