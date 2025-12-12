-- Spam users table
-- Stores FIDs marked as spam (label_value = 0)
-- These users are completely filtered from the feed

CREATE TABLE public.spammy_users (
    id uuid DEFAULT public.generate_ulid() NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    fid bigint NOT NULL,
    source text NOT NULL,  -- Source of the spam label (e.g., 'merkle', 'uno')
    CONSTRAINT spammy_users_pkey PRIMARY KEY (id),
    CONSTRAINT spammy_users_fid_unique UNIQUE (fid)
);

CREATE INDEX spammy_users_fid_index ON public.spammy_users USING btree (fid);
CREATE INDEX spammy_users_created_at_index ON public.spammy_users USING btree (created_at);

-- Add trigger for updated_at
CREATE TRIGGER update_spammy_users_updated_at BEFORE UPDATE ON public.spammy_users
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

-- Nerfed users table
-- Stores FIDs that have been "nerfed" for malicious activity (label_value = 3)
-- These users are not completely filtered out like spam users,
-- but may have reduced visibility or other restrictions applied

CREATE TABLE public.nerfed_users (
    id uuid DEFAULT public.generate_ulid() NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    fid bigint NOT NULL,
    source text NOT NULL,  -- Source of the nerf label (e.g., 'merkle', 'uno')
    CONSTRAINT nerfed_users_pkey PRIMARY KEY (id),
    CONSTRAINT nerfed_users_fid_unique UNIQUE (fid)
);

CREATE INDEX nerfed_users_fid_index ON public.nerfed_users USING btree (fid);
CREATE INDEX nerfed_users_created_at_index ON public.nerfed_users USING btree (created_at);

-- Add trigger for updated_at
CREATE TRIGGER update_nerfed_users_updated_at BEFORE UPDATE ON public.nerfed_users
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();
