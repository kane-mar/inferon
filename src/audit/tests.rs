    use super::*;

    #[test]
    fn a_share_token_is_prefixed_and_unique() {
        let token = mint_token();

        assert!(token.starts_with(TOKEN_PREFIX));
        assert!(token.len() > TOKEN_PREFIX.len() + 30, "high-entropy body");
        assert_ne!(token, mint_token());
    }

    #[tokio::test]
    async fn a_share_resolves_until_it_is_revoked() {
        let store = InMemoryAuditShareStore::default();
        let tenant = Uuid::new_v4();
        let hash = crate::sha256_hex(&mint_token());
        let id = store.create(tenant, "prospect", &hash).await.unwrap();

        assert_eq!(store.resolve(&hash).await.unwrap(), Some(tenant));
        assert_eq!(store.list(tenant).await.unwrap().len(), 1);
        assert!(store.list(Uuid::new_v4()).await.unwrap().is_empty());

        assert!(store.revoke(tenant, id).await.unwrap());
        assert_eq!(store.resolve(&hash).await.unwrap(), None, "revoked");
        assert!(!store.revoke(tenant, id).await.unwrap(), "already revoked");
    }

    #[tokio::test]
    async fn another_tenant_cannot_revoke_a_share() {
        let store = InMemoryAuditShareStore::default();
        let tenant = Uuid::new_v4();
        let hash = crate::sha256_hex("token");
        let id = store.create(tenant, "", &hash).await.unwrap();

        assert!(!store.revoke(Uuid::new_v4(), id).await.unwrap());
        assert!(store.resolve(&hash).await.unwrap().is_some());
    }
