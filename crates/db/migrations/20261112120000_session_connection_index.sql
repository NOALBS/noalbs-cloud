-- Sessions are looked up by the connection they signed in with: a user's
-- sessions, their last activity (the admin's user list asks it per user), and
-- the cascade when a connection is deleted. Without this each of those read
-- every session.
CREATE INDEX user_sessions_oauth2_connection_id_idx ON user_sessions (oauth2_connection_id);
