use std::borrow::Borrow;
use std::ptr::NonNull;

struct Entry<K, V> {
    key: K,
    value: V,
    next: *mut Entry<K, V>,
    prev: *mut Entry<K, V>,
}

pub struct LruCache<K, V> {
    capacity: usize,
    cache: std::collections::HashMap<K, NonNull<Entry<K, V>>>,
    head: *mut Entry<K, V>,
    tail: *mut Entry<K, V>,
}

impl <K, V> Drop for LruCache<K, V> {
    fn drop(&mut self) {
        let mut current = self.head;
        while !current.is_null() {
            let next = unsafe { (*current).next };

            drop(unsafe {
                Box::from_raw(current)
            });

            current = next;
        }
    }
}

impl <K, V> LruCache<K, V>
    where K: std::hash::Hash + std::cmp::Eq + Clone
{
    pub fn new(capacity: usize) -> Self {
        LruCache {
            capacity,
            cache: std::collections::HashMap::with_capacity(capacity),
            head: std::ptr::null_mut(),
            tail: std::ptr::null_mut(),
        }
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    fn remove_entry(&mut self, entry: NonNull<Entry<K, V>>) {
        let prev = unsafe { entry.as_ref().prev };
        let next = unsafe { entry.as_ref().next };

        if !prev.is_null() {
            unsafe { (*prev).next = next };
        } else {
            self.head = next;
        }

        if !next.is_null() {
            unsafe { (*next).prev = prev };
        } else {
            self.tail = prev;
        }
    }

    fn push_entry_front(&mut self, mut entry: NonNull<Entry<K, V>>) {
        unsafe {
            entry.as_mut().next = self.head;
            entry.as_mut().prev = std::ptr::null_mut();
        }

        if !self.head.is_null() {
            unsafe { (*self.head).prev = entry.as_ptr() };
        } else {
            self.tail = entry.as_ptr();
        }

        self.head = entry.as_ptr();
    }

    fn get_free_entry(&mut self, key: K, value: V) -> NonNull<Entry<K, V>> {
        let entry = Box::into_raw(Box::new(Entry {
            key,
            value,
            next: std::ptr::null_mut(),
            prev: std::ptr::null_mut(),
        }));
        NonNull::new(entry).unwrap()
    }

    fn free_entry(&mut self, entry: NonNull<Entry<K, V>>) {
        drop(unsafe {
            Box::from_raw(entry.as_ptr())
        });
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
        where K: Borrow<Q>, Q: std::hash::Hash + Eq + ?Sized,
    {

        if let Some(entry) = self.cache.get(&key).copied() {
            self.remove_entry(entry);
            self.push_entry_front(entry);
            unsafe { Some(&entry.as_ref().value) }
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        let entry = self.get_free_entry(key.clone(), value);
        self.push_entry_front(entry);

        if let Some(old_entry) = self.cache.insert(key, entry) {
            self.remove_entry(old_entry);
            self.free_entry(old_entry);
        }
        else if self.cache.len() > self.capacity {
            let entry = self.tail;

            if let Some(entry) = NonNull::new(entry) {
                self.remove_entry(entry);
                self.cache.remove(unsafe { &entry.as_ref().key });
                self.free_entry(entry);
            }
        }
    }

    pub fn remove<Q>(&mut self, key: &Q)
        where K: Borrow<Q>, Q: std::hash::Hash + Eq + ?Sized,
    {
        if let Some(entry) = self.cache.remove(key) {
            self.remove_entry(entry);
            self.free_entry(entry);
        }
    }
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{self, AtomicIsize};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_insert_with_zero_capacity() {
        let mut cache: LruCache<String, i32> = LruCache::new(0);
        cache.insert("test".to_string(), 42);
        assert_eq!(cache.get("test"), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_get_with_zero_capacity() {
        let mut cache: LruCache<String, i32> = LruCache::new(0);
        assert_eq!(cache.get("test"), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_construct_and_miss() {
        let mut cache: LruCache<String, i32> = LruCache::new(1);
        assert_eq!(cache.get("test"), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut cache: LruCache<String, i32> = LruCache::new(1);
        cache.insert("test".to_string(), 42);
        assert_eq!(cache.get("test"), Some(&42));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_insert_twice_overflow() {
        let mut cache: LruCache<String, i32> = LruCache::new(1);
        cache.insert("old".to_string(), 123);
        cache.insert("test".to_string(), 42);
        assert_eq!(cache.get("test"), Some(&42));
        assert_eq!(cache.get("old"), None);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_least_recent_is_evicted() {
        let mut cache: LruCache<String, i32> = LruCache::new(2);
        cache.insert("old".to_string(), 123);
        cache.insert("test".to_string(), 42);
        assert_eq!(cache.get("old"), Some(&123));
        assert_eq!(cache.len(), 2);

        cache.insert("new".to_string(), 13);
        assert_eq!(cache.get("test"), None);
        assert_eq!(cache.get("old"), Some(&123));
        assert_eq!(cache.get("new"), Some(&13));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_drop_on_evict_works() {
        let counter = Arc::new(AtomicIsize::new(0));

        struct Droppy(Arc<AtomicIsize>);

        impl Drop for Droppy {
            fn drop(&mut self) {
                self.0.fetch_add(1, atomic::Ordering::SeqCst);
            }
        }

        {
            let mut cache: LruCache<String, Droppy> = LruCache::new(1);
            cache.insert("old".to_string(), Droppy(counter.clone()));
            cache.insert("test".to_string(), Droppy(counter.clone()));

            assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
        }

        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
