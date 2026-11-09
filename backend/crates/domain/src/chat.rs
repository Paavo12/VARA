//! The chat's quick replies (PRD T-8): one tap, so nobody has to type at the roadside. The IDs
//! are stored with each message; the server owns the wording so both apps show the same text.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickReply {
    pub id: &'static str,
    pub text: &'static str,
}

pub const RIDER_QUICK_REPLIES: &[QuickReply] = &[
    QuickReply { id: "coming_down", text: "I'm on my way down" },
    QuickReply { id: "at_entrance", text: "I'm by the entrance" },
    QuickReply { id: "running_late", text: "Running 2 min late" },
    QuickReply { id: "thank_you", text: "Thank you!" },
];

pub const DRIVER_QUICK_REPLIES: &[QuickReply] = &[
    QuickReply { id: "arrived", text: "I've arrived" },
    QuickReply { id: "two_minutes", text: "I'm 2 minutes away" },
    QuickReply { id: "traffic", text: "Traffic is heavy, on my way" },
    QuickReply { id: "thank_you", text: "Thank you!" },
];
