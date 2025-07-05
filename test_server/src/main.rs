fn main() {
    std::thread::spawn(|| {
        loop {
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() {
                print!("Echo: {}", input);
                if input.trim() == "exit" {
                    println!("Exiting...");
                    std::process::exit(0);
                }
            }
        }
    });
    let mut i = 0;
    loop {
        println!("Hello, world! {}", i);
        i += 1;
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
