mod container;
mod image;

use container::Container;

use anyhow::Result;
use image::Image;

const ROOT_FS: &str = "/root/cc/fs"; //TODO: Replace with input variable or auto-generate, if we wanna match docker behaivour

// TODO: Use Clap.
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let command = &args[1];
    let command_args = &args[2..];

    match command.as_str() {
        "run" => Container::spawn(command_args, ROOT_FS),
        "pull" => {

            if command_args.len() == 0 {
                anyhow::bail!("Did not receive image for pull")
            }

            let image_str = &command_args[0];
            let image_parts: Vec<_> = image_str.split(':').collect();

            match image_parts[..] {
                [img] => Image::pull(img, "latest", ROOT_FS),
                [img, tag] => Image::pull(img, tag, ROOT_FS),
                _ => anyhow::bail!("{:?} was not in a correct format of image:tag or just image", image_parts)
            }.map(|_| ())
        },
        _ => panic!("Unknown command: {}", command)
    }
}