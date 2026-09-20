-- Elduro kanonisk arkiv i MariaDB (se docs/format/arkivkoding-vurdering.md).
-- Kjøres én gang mot elduro-databasen i den delte mariadb:11.4-containeren
-- (dokumentert unntak fra container-normen, beslutning 20.09.2026):
--   CREATE DATABASE elduro CHARACTER SET utf8mb4;
--   CREATE USER 'elduro'@'%' IDENTIFIED BY '<passord>';
--   GRANT ALL PRIVILEGES ON elduro.* TO 'elduro'@'%';
-- Deretter: mariadb -u elduro -p elduro < db/schema.sql

CREATE TABLE IF NOT EXISTS sessions (
  session_id   VARCHAR(64) PRIMARY KEY,
  subject_id   VARCHAR(32) NOT NULL,
  agent        VARCHAR(32) NOT NULL,
  source       VARCHAR(48) NOT NULL,
  -- v1-forbehold: wire-rammene baerer ikke beltets MAC enna, saa device_id =
  -- source inntil firmware/agent melder belteidentitet (se vurderingen kap. 5).
  device_id    VARCHAR(48) NOT NULL,
  device_name  VARCHAR(64),
  mode         ENUM('ecg','hrv','hr') NOT NULL DEFAULT 'ecg',
  started_ns   BIGINT UNSIGNED NOT NULL,
  ended_ns     BIGINT UNSIGNED,
  clock        ENUM('ntp-synced','unsynced','host') NOT NULL DEFAULT 'host',
  origin       ENUM('live','spill','migrert-v1') NOT NULL,
  notes        TEXT
);

CREATE TABLE IF NOT EXISTS frames (
  device_id    VARCHAR(48)     NOT NULL,
  stream       ENUM('ecg','acc','hr') NOT NULL,
  ts_device_ns BIGINT UNSIGNED NOT NULL,
  session_id   VARCHAR(64)     NOT NULL,
  seq          INT UNSIGNED    NOT NULL DEFAULT 0,
  ts_host_ns   BIGINT UNSIGNED NOT NULL,
  n_samples    SMALLINT UNSIGNED NOT NULL,
  payload      JSON            NOT NULL,
  -- Dedup-noekkelen fra frame-schema (frosset): idempotent merge via
  -- INSERT IGNORE er en egenskap av denne noekkelen.
  PRIMARY KEY (device_id, stream, ts_device_ns),
  KEY by_session (session_id, stream, seq),
  KEY by_time (device_id, stream, ts_host_ns)
);

CREATE TABLE IF NOT EXISTS annotations (
  id           BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  subject_id   VARCHAR(32) NOT NULL,
  t_start_ns   BIGINT UNSIGNED NOT NULL,
  t_end_ns     BIGINT UNSIGNED NOT NULL,
  label        VARCHAR(48) NOT NULL,
  value        TEXT,
  source       ENUM('self-live','self-review','clinician','algo') NOT NULL,
  confidence   FLOAT,
  entry_latency_ms INT UNSIGNED,
  entry_method VARCHAR(32),
  KEY by_subject_time (subject_id, t_start_ns)
);
