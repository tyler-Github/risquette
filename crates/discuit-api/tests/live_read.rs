//! Read-only smoke test against the live instance. GET requests only.
//! Run explicitly with: `cargo test -p discuit-api --test live_read -- --ignored`

use discuit_api::{Client, FeedQuery, FeedSort, FeedType, DEFAULT_BASE_URL};

#[test]
#[ignore = "hits the network"]
fn decodes_live_read_endpoints() {
    let c = Client::new(DEFAULT_BASE_URL).unwrap();
    let initial = c.initial().expect("initial");
    for sort in FeedSort::ALL {
        let page = c.feed(&FeedQuery { feed: FeedType::All, sort, limit: Some(10), ..Default::default() }).unwrap_or_else(|e| panic!("feed {sort:?}: {e}"));
        // Follow one page to make sure the cursor round-trips.
        if let Some(next) = page.next.clone() {
            c.feed(&FeedQuery { feed: FeedType::All, sort, limit: Some(10), next: Some(next), ..Default::default() })
                .unwrap_or_else(|e| panic!("feed {sort:?} page 2: {e}"));
        }
        eprintln!("{sort:?}: ok ({} posts)", page.posts.map_or(0, |p| p.len()));
    }
    let community = initial.communities.unwrap_or_default().into_iter().next().expect("a default community");
    let full = c.community_by_name(&community.name).expect("community");
    let page = c.feed(&FeedQuery { feed: FeedType::Community, community_id: Some(full.id.clone()), limit: Some(10), ..Default::default() }).expect("community feed");
    let post = page.posts.unwrap_or_default().into_iter().next().expect("a post");
    let detail = c.post(&post.public_id).expect("post");
    c.comments(&post.public_id, None).expect("comments");
    let user = c.user(&detail.username).expect("user");
    let ufeed = c.user_feed(&user.username, None, Some(10), None).expect("user feed");
    if let Some(next) = ufeed.next {
        c.user_feed(&user.username, None, Some(10), Some(&next)).expect("user feed page 2");
    }
    c.user_lists(&user.username, None, None).expect("lists");
    c.communities(Some("all"), None, None, Some(20)).expect("communities");
    c.communities(Some("all"), Some("a"), None, Some(20)).expect("community search");
    c.rules(&full.id).expect("rules");
    c.mods(&full.id).expect("mods");
}

#[test]
#[ignore = "hits the network"]
fn community_feed_sorts() {
    let c = Client::new(DEFAULT_BASE_URL).unwrap();
    let community = c.community_by_name("Discuit").expect("community");
    for sort in FeedSort::COMMUNITY {
        c.feed(&FeedQuery { feed: FeedType::Community, sort, community_id: Some(community.id.clone()), limit: Some(5), ..Default::default() })
            .unwrap_or_else(|e| panic!("community feed {sort:?}: {e}"));
    }
    // A known image post decodes its images.
    let page = c.feed(&FeedQuery { feed: FeedType::All, sort: FeedSort::TopWeek, limit: Some(25), ..Default::default() }).unwrap();
    for p in page.posts.unwrap_or_default().into_iter().filter(|p| p.kind == discuit_api::PostType::Image).take(3) {
        let full = c.post(&p.public_id).unwrap();
        assert!(full.primary_image().is_some(), "image post {} has no image", p.public_id);
    }
}
