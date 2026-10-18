-- The owner can hand the workspace to one of its members.
INSERT INTO permissions (permission, description)
VALUES ('workspace.transfer', 'Make another member the owner');
INSERT INTO workspace_role_permissions (role, permission)
VALUES ('owner', 'workspace.transfer');

-- A transfer removes the previous owner's cards from the workspace's Stripe
-- customer. Until the new owner adds one, the dashboard asks them to; cleared
-- once Stripe has a card again.
ALTER TABLE workspaces ADD COLUMN needs_payment_method BOOLEAN NOT NULL DEFAULT false;
