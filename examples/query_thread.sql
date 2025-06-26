-- Example queries for finding all casts in a thread/conversation

-- 1. Find all casts in a thread given a cast hash
-- This will return all casts that share the same root parent
SELECT 
    c.hash,
    c.fid,
    c.text,
    c.timestamp,
    c.parent_hash,
    c.parent_url
FROM casts c
WHERE c.root_parent_hash = (
    SELECT root_parent_hash 
    FROM casts 
    WHERE hash = '\x1234abcd'::bytea -- Replace with actual cast hash
)
AND c.deleted_at IS NULL
ORDER BY c.timestamp ASC;

-- 2. Find all threads started by a specific FID
SELECT DISTINCT
    c.root_parent_hash,
    c.root_parent_fid,
    c.root_parent_url,
    COUNT(*) OVER (PARTITION BY c.root_parent_hash) as reply_count
FROM casts c
WHERE c.root_parent_fid = 12345 -- Replace with actual FID
AND c.deleted_at IS NULL
ORDER BY reply_count DESC;

-- 3. Find all casts in threads that a user has participated in
SELECT DISTINCT
    c2.*
FROM casts c1
INNER JOIN casts c2 ON c1.root_parent_hash = c2.root_parent_hash
WHERE c1.fid = 12345 -- Replace with user's FID
AND c1.deleted_at IS NULL
AND c2.deleted_at IS NULL
ORDER BY c2.timestamp DESC;

-- 4. Mute all casts from a specific thread
-- This query can be used to identify casts to hide from notifications
SELECT 
    c.hash,
    c.fid
FROM casts c
WHERE c.root_parent_hash = '\x5678efgh'::bytea -- Replace with thread's root hash
AND c.deleted_at IS NULL
AND c.timestamp > NOW() - INTERVAL '7 days'; -- Only recent casts

-- 5. Find the most active threads (by reply count)
SELECT 
    c.root_parent_hash,
    c.root_parent_fid,
    COUNT(*) as total_replies,
    COUNT(DISTINCT c.fid) as unique_participants,
    MAX(c.timestamp) as last_activity
FROM casts c
WHERE c.root_parent_hash IS NOT NULL
AND c.deleted_at IS NULL
AND c.timestamp > NOW() - INTERVAL '24 hours'
GROUP BY c.root_parent_hash, c.root_parent_fid
ORDER BY total_replies DESC
LIMIT 100;

-- 6. Get the full thread hierarchy for a cast
WITH RECURSIVE thread_tree AS (
    -- Start with the root cast
    SELECT 
        c.hash,
        c.fid,
        c.text,
        c.parent_hash,
        c.timestamp,
        0 as depth
    FROM casts c
    WHERE c.hash = (
        SELECT COALESCE(root_parent_hash, hash)
        FROM casts
        WHERE hash = '\x9999aaaa'::bytea -- Replace with any cast in the thread
    )
    
    UNION ALL
    
    -- Recursively find all children
    SELECT 
        c.hash,
        c.fid,
        c.text,
        c.parent_hash,
        c.timestamp,
        tt.depth + 1
    FROM casts c
    INNER JOIN thread_tree tt ON c.parent_hash = tt.hash
    WHERE c.deleted_at IS NULL
)
SELECT * FROM thread_tree
ORDER BY depth, timestamp;