//! LIFO compensation planning, exactly-once guards, and escalation rules.
//!
//! Prior succeeded steps are compensated LIFO, each exactly-once; terminal
//! compensation failure escalates to the manual review queue.
