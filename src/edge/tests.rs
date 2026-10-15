    use super::*;

    #[tokio::test]
    async fn enroll_authenticate_touch_and_revoke() {
        let store = InMemoryEdgeNodeStore::default();
        let tenant = Uuid::new_v4();
        let token = mint_edge_token();
        let hash = crate::sha256_hex(&token);
        let id = store.enroll(tenant, "node-1", &hash).await.unwrap();
        assert_eq!(store.authenticate(&hash).await.unwrap(), Some((id, tenant)));
        store.touch(id).await.unwrap();
        let nodes = store.list(tenant).await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].last_seen_at.is_some());
        // Revoking stops the credential immediately.
        assert!(store.revoke(tenant, id).await.unwrap());
        assert_eq!(store.authenticate(&hash).await.unwrap(), None);
        assert!(store.list(tenant).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_node_cannot_be_revoked_by_another_tenant() {
        let store = InMemoryEdgeNodeStore::default();
        let tenant = Uuid::new_v4();
        let id = store.enroll(tenant, "n", "hash").await.unwrap();
        assert!(!store.revoke(Uuid::new_v4(), id).await.unwrap(), "wrong tenant");
        assert_eq!(store.authenticate("hash").await.unwrap(), Some((id, tenant)));
    }
