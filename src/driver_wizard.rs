use inquire::{InquireError, Select, Text};
use log::warn;

pub fn wizard() {
    warn!("This isn't working yet. Don't use it!");
    let options: Vec<&str> = vec![
        "Serial",
        "UDP",
        // "Serial with webserver", // TODO
        // "UDP with webserver",
        // "Animation",
    ];

    let ans: Result<&str, InquireError> =
        Select::new("How will you communicate with the controller?", options).prompt();

    let _template = match ans {
        Ok(choice) => choice,
        Err(e) => panic!("Could not select a controller communication mode: {e}"),
    };

    let led_count = Text::new("How many LEDs are you using?").prompt();

    let _led_count = match led_count {
        Ok(name) => name,
        Err(_) => panic!("An error happened when asking for your name, try again later."),
    };

    let strand_count = Text::new("How many LED strands are you using?").prompt();

    let _strand_count = match strand_count {
        Ok(name) => name,
        Err(_) => panic!("An error happened when asking for your name, try again later."),
    };

    let mut led_pins = Vec::new();

    for i in 1.._led_count.parse::<i32>().unwrap() {
        let pin = Text::new(&format!("Enter pin for LED {i}")).prompt();

        match pin {
            Ok(pin) => led_pins.push(pin),
            Err(_) => panic!("An error happened when asking for your name, try again later."),
        };
    }
}
