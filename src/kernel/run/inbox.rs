use crate::sessions::Message;

pub enum InboxItem {
    Message(Message),
    Yield,
}
