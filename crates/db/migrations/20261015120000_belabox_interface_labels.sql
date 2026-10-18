-- Names streamers give their BELABOX's network interfaces, by the interface's
-- key (a modem's "model - last digits of its IMEI", a WiFi adapter's hardware,
-- else the interface name), e.g. {"EM7455 - 12345": "Phone A"}.
ALTER TABLE switchers
    ADD COLUMN belabox_interface_labels JSONB NOT NULL DEFAULT '{}';
