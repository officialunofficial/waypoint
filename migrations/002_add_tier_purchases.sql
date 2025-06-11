-- Add tier_purchases table for Farcaster Pro tier support
CREATE TABLE public.tier_purchases
(
    id               uuid                     DEFAULT public.generate_ulid() NOT NULL,
    created_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    updated_at       timestamp with time zone DEFAULT CURRENT_TIMESTAMP      NOT NULL,
    "timestamp"      timestamp with time zone                                NOT NULL,
    deleted_at       timestamp with time zone,
    fid              bigint                                                  NOT NULL,
    tier_type        smallint                                                NOT NULL, -- 0=None, 1=Pro
    for_days         bigint                                                  NOT NULL,
    payer            bytea                                                   NOT NULL,
    block_number     bigint                                                  NOT NULL,
    block_hash       bytea                                                   NOT NULL,
    log_index        integer                                                 NOT NULL,
    tx_index         integer                                                 NOT NULL,
    tx_hash          bytea                                                   NOT NULL,
    block_timestamp  timestamp with time zone                                NOT NULL,
    chain_id         bigint                                                  NOT NULL,
    CONSTRAINT tier_purchases_pkey PRIMARY KEY (id),
    CONSTRAINT tier_purchases_tx_log_unique UNIQUE (tx_hash, log_index)
);

-- Create indexes for tier_purchases
CREATE INDEX tier_purchases_fid_index ON public.tier_purchases USING btree (fid);
CREATE INDEX tier_purchases_timestamp_index ON public.tier_purchases USING btree ("timestamp");
CREATE INDEX tier_purchases_tier_type_index ON public.tier_purchases USING btree (tier_type);
CREATE INDEX tier_purchases_payer_index ON public.tier_purchases USING btree (payer);
CREATE INDEX tier_purchases_block_number_index ON public.tier_purchases USING btree (block_number);

-- Add trigger for updated_at column
CREATE TRIGGER update_tier_purchases_updated_at BEFORE UPDATE ON public.tier_purchases
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();