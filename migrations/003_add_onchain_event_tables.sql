-- Add specific tables for onchain event types that were previously only stored in generic onchain_events table

-- Signer Events Table (EVENT_TYPE_SIGNER = 1)
-- Based on SignerEventBody proto definition
CREATE TABLE public.signer_events
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    key              bytea                                                   NOT NULL,
    key_type         smallint                                                NOT NULL,
    event_type       smallint                                                NOT NULL, -- ADD=1, REMOVE=2, ADMIN_RESET=3
    metadata         bytea,
    metadata_type    smallint,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT signer_events_pkey PRIMARY KEY (id),
    CONSTRAINT signer_events_tx_log_unique UNIQUE (tx_hash, log_index)
);

-- Create indexes for signer_events
CREATE INDEX signer_events_fid_index ON public.signer_events USING btree (fid);
CREATE INDEX signer_events_timestamp_index ON public.signer_events USING btree ("timestamp");
CREATE INDEX signer_events_event_type_index ON public.signer_events USING btree (event_type);
CREATE INDEX signer_events_key_index ON public.signer_events USING btree (key);
CREATE INDEX signer_events_block_number_index ON public.signer_events USING btree (block_number);

-- Signer Migrated Events Table (EVENT_TYPE_SIGNER_MIGRATED = 2)  
-- Based on SignerMigratedEventBody proto definition
CREATE TABLE public.signer_migrated_events
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    migrated_at      bigint                                                  NOT NULL,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT signer_migrated_events_pkey PRIMARY KEY (id),
    CONSTRAINT signer_migrated_events_tx_log_unique UNIQUE (tx_hash, log_index)
);

-- Create indexes for signer_migrated_events
CREATE INDEX signer_migrated_events_fid_index ON public.signer_migrated_events USING btree (fid);
CREATE INDEX signer_migrated_events_timestamp_index ON public.signer_migrated_events USING btree ("timestamp");
CREATE INDEX signer_migrated_events_migrated_at_index ON public.signer_migrated_events USING btree (migrated_at);
CREATE INDEX signer_migrated_events_block_number_index ON public.signer_migrated_events USING btree (block_number);

-- ID Register Events Table (EVENT_TYPE_ID_REGISTER = 3)
-- Based on IdRegisterEventBody proto definition
CREATE TABLE public.id_register_events
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    to_address       bytea                                                   NOT NULL,
    event_type       smallint                                                NOT NULL, -- REGISTER=1, TRANSFER=2, CHANGE_RECOVERY=3
    from_address     bytea,
    recovery_address bytea,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT id_register_events_pkey PRIMARY KEY (id),
    CONSTRAINT id_register_events_tx_log_unique UNIQUE (tx_hash, log_index)
);

-- Create indexes for id_register_events
CREATE INDEX id_register_events_fid_index ON public.id_register_events USING btree (fid);
CREATE INDEX id_register_events_timestamp_index ON public.id_register_events USING btree ("timestamp");
CREATE INDEX id_register_events_event_type_index ON public.id_register_events USING btree (event_type);
CREATE INDEX id_register_events_to_address_index ON public.id_register_events USING btree (to_address);
CREATE INDEX id_register_events_from_address_index ON public.id_register_events USING btree (from_address) WHERE (from_address IS NOT NULL);
CREATE INDEX id_register_events_block_number_index ON public.id_register_events USING btree (block_number);

-- Storage Rent Events Table (EVENT_TYPE_STORAGE_RENT = 4)
-- Based on StorageRentEventBody proto definition
CREATE TABLE public.storage_rent_events
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    payer            bytea                                                   NOT NULL,
    units            integer                                                 NOT NULL,
    expiry           integer                                                 NOT NULL,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT storage_rent_events_pkey PRIMARY KEY (id),
    CONSTRAINT storage_rent_events_tx_log_unique UNIQUE (tx_hash, log_index)
);

-- Create indexes for storage_rent_events
CREATE INDEX storage_rent_events_fid_index ON public.storage_rent_events USING btree (fid);
CREATE INDEX storage_rent_events_timestamp_index ON public.storage_rent_events USING btree ("timestamp");
CREATE INDEX storage_rent_events_payer_index ON public.storage_rent_events USING btree (payer);
CREATE INDEX storage_rent_events_expiry_index ON public.storage_rent_events USING btree (expiry);
CREATE INDEX storage_rent_events_block_number_index ON public.storage_rent_events USING btree (block_number);

-- Add triggers for updated_at columns
CREATE TRIGGER update_signer_events_updated_at BEFORE UPDATE ON public.signer_events
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_signer_migrated_events_updated_at BEFORE UPDATE ON public.signer_migrated_events
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_id_register_events_updated_at BEFORE UPDATE ON public.id_register_events
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_storage_rent_events_updated_at BEFORE UPDATE ON public.storage_rent_events
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();