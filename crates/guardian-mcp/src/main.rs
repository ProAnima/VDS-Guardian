fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    guardian_mcp::run(&arguments)
}
