use eframe::egui::{Event, Key};

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Previous,
    Next,
    ShiftSpreadBackward,
    ShiftSpreadForward,
    FirstPrefix,
    Last,
    OpenFile,
    OpenFolder,
    Spread,
    Direction,
    SelectOther,
    Move(char),
    Undo,
    Slideshow,
    Top,
    Fullscreen,
    Help,
    Escape,
    Quit,
    Unbound,
}

impl Action {
    pub(crate) fn for_char(ch: char) -> Option<Self> {
        Some(match ch {
            'h' | 'k' => Self::Previous,
            'j' | 'l' => Self::Next,
            'g' => Self::FirstPrefix,
            'G' => Self::Last,
            'o' => Self::OpenFile,
            'O' => Self::OpenFolder,
            'v' => Self::Spread,
            'r' => Self::Direction,
            'u' => Self::Undo,
            's' => Self::Slideshow,
            't' => Self::Top,
            'f' => Self::Fullscreen,
            'q' => Self::Quit,
            _ => return None,
        })
    }

    pub(crate) fn from_event(event: &Event) -> Option<(Self, bool)> {
        let Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } = event
        else {
            return None;
        };
        if modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
            return None;
        }
        let action = match key {
            Key::Tab => Self::SelectOther,
            Key::OpenBracket if !modifiers.shift => Self::ShiftSpreadBackward,
            Key::CloseBracket if !modifiers.shift => Self::ShiftSpreadForward,
            Key::Escape => Self::Escape,
            Key::F1 => Self::Help,
            Key::Questionmark => Self::Help,
            Key::Slash if modifiers.shift => Self::Help,
            key => match key.name().as_bytes() {
                [letter] if letter.is_ascii_alphabetic() => {
                    let ch = char::from(if modifiers.shift {
                        letter.to_ascii_uppercase()
                    } else {
                        letter.to_ascii_lowercase()
                    });
                    Self::for_char(ch).unwrap_or(Self::Move(ch))
                }
                [digit] if digit.is_ascii_digit() && !modifiers.shift => {
                    Self::Move(char::from(*digit))
                }
                _ => Self::Unbound,
            },
        };
        if *repeat && !matches!(action, Self::Next | Self::Previous) {
            return None;
        }
        Some((action, *repeat))
    }
}
