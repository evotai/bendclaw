use crate::kernel::Message;

pub enum InboxItem {
    Message(Message),
    Yield,
}
