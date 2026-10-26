-- #147: public share links are removed entirely (owner decision). A share
-- token gave unauthenticated read access to a node, and it was too easy to
-- publish personal data by accident. Every existing link stops working with
-- this migration, which is intended. Only the random tokens are lost; the
-- nodes they pointed at are untouched.
DROP TABLE IF EXISTS share_tokens;
