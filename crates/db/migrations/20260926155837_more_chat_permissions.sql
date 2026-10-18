-- Chat command roles for chatters with the channel's subscriber or VIP badge,
-- ranked between the public and moderators (the enum's order matches Rust's).
ALTER TYPE chat_command_roles ADD VALUE 'SUBSCRIBER' BEFORE 'MODERATOR';
ALTER TYPE chat_command_roles ADD VALUE 'VIP' BEFORE 'MODERATOR';
