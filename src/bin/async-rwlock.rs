use async_std::sync::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let x = Arc::new(RwLock::new(false));
    let x2 = x.clone();
    let x3 = x.clone();
    tokio::spawn(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(1000)).await;
        println!("updating");
        let mut w = x3.write().await;
        *w = true;
    });

    tokio::spawn(async move {
        loop {
            {
                let r = x2.read().await;
                if *r {
                    println!("is true");
                    break;
                } else {
                    println!("is false");
                }
            }
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
        }
    })
    .await;
}

// works
//
// // use async_std::sync::RwLock;
// use std::sync::Arc;
// use std::sync::RwLock;

// #[tokio::main]
// async fn main() {
//     let x = Arc::new(RwLock::new(false));
//     let x2 = x.clone();
//     let x3 = x.clone();
//     tokio::spawn(async move {
//         tokio::time::delay_for(std::time::Duration::from_millis(1000)).await;
//         println!("updating");
//         let mut w = x3.write().unwrap();
//         *w = true;
//     });

//     tokio::spawn(async move {
//         loop {
//             {
//                 let r = x2.read().unwrap();
//             if *r {
//                 println!("is true");
//                 break;
//             } else {
//                 println!("is false");
//             }
//         }
//             tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
//         }
//     })
//     .await;
// }
