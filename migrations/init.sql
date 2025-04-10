-- Enable required extensions
CREATE
EXTENSION IF NOT EXISTS pgcrypto;
CREATE
EXTENSION IF NOT EXISTS vector;

-- Create UUID generation function
CREATE FUNCTION public.generate_ulid() RETURNS uuid
    LANGUAGE sql STRICT PARALLEL SAFE
    AS $$
SELECT (lpad(to_hex((floor((EXTRACT(epoch FROM clock_timestamp()) * (1000)::numeric)))::bigint), 12, '0'::text) ||
        encode(public.gen_random_bytes(10), 'hex'::text)) ::uuid
$$;

-- Create updated_at trigger function
CREATE FUNCTION public.update_updated_at_column() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
   NEW.updated_at
= CURRENT_TIMESTAMP;
RETURN NEW;
END;
$$;

-- Create base tables
CREATE TABLE public.events
(
    id bigint PRIMARY KEY
);

CREATE TABLE public.messages
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    pruned_at        timestamp with time zone,
    revoked_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    type             smallint                                                NOT NULL,
    hash_scheme      smallint                                                NOT NULL,
    signature_scheme smallint                                                NOT NULL,
    hash             bytea                                                   NOT NULL,
    signer           bytea                                                   NOT NULL,
    body             json                                                    NOT NULL,
    raw              bytea                                                   NOT NULL,
    CONSTRAINT messages_pkey PRIMARY KEY (id),
    CONSTRAINT messages_hash_unique UNIQUE (hash),
    CONSTRAINT messages_hash_fid_type_unique UNIQUE (hash, fid, type)
);

CREATE TABLE public.casts
(
    id                 uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at         timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at         timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"        timestamp with time zone                                NOT NULL,
    deleted_at         timestamp with time zone,
    fid                bigint                                                  NOT NULL,
    parent_fid         bigint,
    hash               bytea                                                   NOT NULL,
    parent_hash        bytea,
    parent_url         text,
    text               text,
    embeds             json                     DEFAULT '[]'::json NOT NULL,
    mentions           json                     DEFAULT '[]'::json NOT NULL,
    mentions_positions json                     DEFAULT '[]'::json NOT NULL,
    type               smallint                 DEFAULT 0                      NOT NULL,
    count_likes        int                      DEFAULT 0                      NOT NULL,
    count_recasts      int                      DEFAULT 0                      NOT NULL,
    CONSTRAINT casts_pkey PRIMARY KEY (id),
    CONSTRAINT casts_hash_key UNIQUE (hash)
);

CREATE TABLE public.reactions
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    target_cast_fid  bigint,
    type             smallint                                                NOT NULL,
    hash             bytea                                                   NOT NULL,
    target_cast_hash bytea,
    target_url       text,
    CONSTRAINT reactions_pkey PRIMARY KEY (id),
    CONSTRAINT reactions_hash_key UNIQUE (hash)
);

CREATE TABLE public.links
(
    id                uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at        timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at        timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"       timestamp with time zone                                NOT NULL,
    deleted_at        timestamp with time zone,
    fid               bigint                                                  NOT NULL,
    target_fid        bigint                                                  NOT NULL,
    display_timestamp timestamp with time zone,
    type              text                                                    NOT NULL,
    hash              bytea                                                   NOT NULL,
    CONSTRAINT links_pkey PRIMARY KEY (id),
    CONSTRAINT links_hash_key UNIQUE (hash)
);

CREATE TABLE public.user_data (
                                  id uuid DEFAULT public.generate_ulid() NOT NULL,
                                  created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
                                  updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
                                  "timestamp" timestamp with time zone NOT NULL,
                                  deleted_at timestamp with time zone,
                                  fid bigint NOT NULL,
                                  type smallint NOT NULL,
                                  hash bytea NOT NULL,
                                  value text NOT NULL,
                                  CONSTRAINT user_data_pkey PRIMARY KEY (id),
                                  CONSTRAINT user_data_fid_type_unique UNIQUE (fid, type)
);

CREATE TABLE public.verifications
(
    id             uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at     timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at     timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"    timestamp with time zone                                NOT NULL,
    deleted_at     timestamp with time zone,
    fid            bigint                                                  NOT NULL,
    hash           bytea                                                   NOT NULL,
    signer_address bytea                                                   NOT NULL,
    block_hash     bytea                                                   NOT NULL,
    signature      bytea                                                   NOT NULL,
    protocol       smallint,
    CONSTRAINT verifications_pkey PRIMARY KEY (id),
    CONSTRAINT verifications_signer_address_fid_unique UNIQUE (signer_address, fid)
);

CREATE TABLE public.username_proofs
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    username         text                                                    NOT NULL,
    type             smallint                                                NOT NULL,
    signature        bytea                                                   NOT NULL,
    owner            bytea,
    CONSTRAINT username_proofs_pkey PRIMARY KEY (id),
    CONSTRAINT username_proofs_username_timestamp_unique UNIQUE (username, "timestamp")
);

-- Create indexes
CREATE INDEX messages_fid_index ON public.messages USING btree (fid);
CREATE INDEX messages_hash_index ON public.messages USING btree (hash);
CREATE INDEX messages_signer_index ON public.messages USING btree (signer);
CREATE INDEX messages_timestamp_index ON public.messages USING btree ("timestamp");
CREATE INDEX messages_fid_timestamp_type_idx ON public.messages USING btree (fid, "timestamp", type)
    WHERE ((pruned_at IS NULL) AND (revoked_at IS NULL) AND (deleted_at IS NULL));

CREATE INDEX casts_fid_timestamp ON public.casts USING btree (fid, "timestamp");
CREATE INDEX casts_parent_hash_index ON public.casts USING btree (parent_hash) WHERE (parent_hash IS NOT NULL);
CREATE INDEX casts_parent_url_index ON public.casts USING btree (parent_url) WHERE (parent_url IS NOT NULL);
CREATE INDEX casts_timestamp_index ON public.casts USING btree ("timestamp");
CREATE INDEX idx_casts_feed ON public.casts USING btree (fid, parent_hash, deleted_at, "timestamp" DESC, hash)
    WHERE ((deleted_at IS NULL) AND (parent_hash IS NULL));

CREATE INDEX reactions_fid_type_target_cast_hash_index ON public.reactions USING btree (fid, type, target_cast_hash);
CREATE INDEX reactions_target_cast_hash_index ON public.reactions USING btree (target_cast_hash) WHERE (target_cast_hash IS NOT NULL);
CREATE INDEX reactions_target_url_index ON public.reactions USING btree (target_url) WHERE (target_url IS NOT NULL);

CREATE INDEX links_fid_index ON public.links USING btree (fid);
CREATE INDEX links_hash_index ON public.links USING btree (hash);
CREATE INDEX links_type_index ON public.links USING btree (type);
CREATE INDEX links_target_fid_index ON public.links USING btree (target_fid);
CREATE INDEX idx_links_followers ON public.links USING btree (fid, target_fid, type, deleted_at)
    WHERE ((deleted_at IS NULL) AND (type = 'follow'::text));

CREATE INDEX user_data_fid_index ON public.user_data USING btree (fid);
CREATE INDEX user_data_timestamp_index ON public.user_data USING btree ("timestamp");
CREATE INDEX user_data_type_index ON public.user_data USING btree (type);

CREATE INDEX verifications_fid_timestamp_index ON public.verifications USING btree (fid, "timestamp");

CREATE INDEX username_proofs_fid_index ON public.username_proofs USING btree (fid);
CREATE INDEX username_proofs_username_index ON public.username_proofs USING btree (username);
CREATE INDEX username_proofs_timestamp_index ON public.username_proofs USING btree ("timestamp");


CREATE TABLE public.onchain_events
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    hash             bytea                                                   NOT NULL,
    type             smallint                                                NOT NULL,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    signer_address   bytea,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT onchain_events_pkey PRIMARY KEY (id),
    CONSTRAINT onchain_events_hash_key UNIQUE (hash)
);

CREATE INDEX onchain_events_fid_index ON public.onchain_events USING btree (fid);
CREATE INDEX onchain_events_type_index ON public.onchain_events USING btree (type);
CREATE INDEX onchain_events_timestamp_index ON public.onchain_events USING btree ("timestamp");


-- Add triggers for updated_at columns
CREATE TRIGGER update_casts_updated_at BEFORE UPDATE ON public.casts
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_reactions_updated_at BEFORE UPDATE ON public.reactions
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_links_updated_at BEFORE UPDATE ON public.links
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_user_data_updated_at BEFORE UPDATE ON public.user_data
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_verifications_updated_at BEFORE UPDATE ON public.verifications
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_username_proofs_updated_at BEFORE UPDATE ON public.username_proofs
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER update_onchain_events_updated_at BEFORE UPDATE ON public.onchain_events
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

