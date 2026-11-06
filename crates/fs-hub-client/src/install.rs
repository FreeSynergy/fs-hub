//! The "no hub found" message.
//!
//! Pattern: Strategy placeholder — today only text; a real installer would plug in here.

/// What to say to the user when there is no hub (ADR 0019: the program asks whether to install it
/// and keeps running alone on "no"). Installing is not implemented yet; this only returns the message.
pub fn offer_install() -> String {
    "No FreeSynergy hub was found on this device. The hub lets your programs share their work \
     (for example a contact from Contacts showing up in Calendar). Install it now? \
     Automatic installation is not available yet; this program keeps working on its own and \
     hands over its history as soon as a hub is running."
        .to_string()
}
