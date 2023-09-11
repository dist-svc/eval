
use std::future::Future;
use std::task::{Poll, Context};
use std::pin::Pin;
use std::marker::PhantomData;

use futures::FutureExt;

pub struct JoinAllWindowed<F, I> {
    f: Vec<(F, Option<I>)>,
    window: usize,
}

pub fn join_all_windowed<F, I>(mut f: Vec<F>, window: usize) -> JoinAllWindowed<F, I>
where
    F: core::future::Future<Output=I>
{
    JoinAllWindowed {
        f: f.drain(..).map(|f| (f, None)).collect(),
        window,
    }
}

impl <F, I> Future for JoinAllWindowed<F, I>
where 
    F: Future<Output=I> + Unpin,
    I: Unpin
{
    type Output = Vec<I>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut polled = 0;
        for i in 0..self.f.len() {
            // Skip already resolved futures
            if self.f[i].1.is_some() {
                continue;
            }

            // Poll unresolved futures
            if let Poll::Ready(v) = self.f[i].0.poll_unpin(cx) {
                // Store result
                self.f[i].1 = Some(v);

                // Decrease poll to keep the window moving
                if polled > 0 {
                    polled -= 1;
                }

                continue;
            }

            // Increment polled count and continue
            polled += 1;
            if polled >= self.window {
                break;
            }
        }

        let pending = self.f.iter().filter(|(_f, i)| i.is_none() ).count();
        if pending > 0 {
            Poll::Pending
        } else {
            Poll::Ready(self.f.drain(..).map(|(_f, i)| i.unwrap() ).collect())
        }
    }
}


pub struct TryJoinAllWindowed<F, I, E> {
    f: Vec<(F, Option<I>)>,
    window: usize,
    _e: PhantomData<E>,
}

pub fn try_join_all_windowed<F, I, E>(mut f: Vec<F>, window: usize) -> TryJoinAllWindowed<F, I, E>
where
    F: core::future::Future<Output=Result<I, E>>
{
    TryJoinAllWindowed {
        f: f.drain(..).map(|f| (f, None)).collect(),
        window,
        _e: PhantomData,
    }
}

impl <F, I, E> Future for TryJoinAllWindowed<F, I, E>
where 
    F: Future<Output=Result<I, E>> + Unpin,
    I: Unpin,
    E: Unpin,
{
    type Output = Result<Vec<I>, E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut polled = 0;
        for i in 0..self.f.len() {
            // Skip already resolved futures
            if self.f[i].1.is_some() {
                continue;
            }

            // Poll unresolved futures
            match self.f[i].0.poll_unpin(cx) {

                Poll::Ready(Ok(v)) => {
                    // Store result
                    self.f[i].1 = Some(v);

                    // Decrease poll to keep the window moving
                    if polled > 0 {
                        polled -= 1;
                    }

                    continue;
                },
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                _ => (),
            }

            // Increment polled count and continue
            polled += 1;
            if polled >= self.window {
                break;
            }
        }

        let pending = self.f.iter().filter(|(_f, i)| i.is_none() ).count();
        if pending > 0 {
            Poll::Pending
        } else {
            Poll::Ready(Ok(self.f.drain(..).map(|(_f, i)| i.unwrap() ).collect()))
        }
    }
}


pub struct FutureWindow<I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> {
    /// Parallelisation factor
    n: usize,

    /// Iterator over inputs for parallel execution
    inputs: I,

    /// Future to be executed per iterator item
    f: F,

    /// Currently executing futures
    current: Vec<Box<R>>,

    /// Completed executor results
    results: Vec<<R as Future>::Output>,
}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> Unpin for FutureWindow<I, R, F> {}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> FutureWindow<I, R, F> 
where
    R: Unpin,
    //<I as Iterator>::Item: core::fmt::Debug,
    //<R as Future>::Output: core::fmt::Debug,
{
    pub fn new(n: usize, inputs: I, f: F) -> Self {
        Self {
            n,
            inputs,
            f,
            current: Vec::new(),
            results: Vec::new(),
        }
    }

    fn update(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Vec<<R as Future>::Output>> {
        let mut pending_tasks = true;

        // Ensure we're running n tasks
        while self.current.len() < self.n {
            match self.inputs.next() {
                // If we have remaining values, start tasks
                Some(v) => {
                    let f = (self.f)(v);
                    self.current.push(Box::new(f));
                    cx.waker().clone().wake();
                },
                // Otherwise, skip this
                None => {
                    pending_tasks = false;
                    break;
                },
            }
        }

        // Poll for completion of current tasks
        let mut current: Vec<_> = self.current.drain(..).collect();

        for mut c in current.drain(..) {
            match c.poll_unpin(cx) {
                Poll::Ready(v) => {
                    // Store result and drop future
                    self.results.push(v);
                },
                Poll::Pending => {
                    // Keep tracking future
                    self.current.push(c);
                },
            }
        }

        // Complete when we have no pending tasks and the current list is empty
        if self.current.is_empty() && !pending_tasks {
            Poll::Ready(self.results.drain(..).collect())

        // Force wake if any tasks have completed but we still have some pending
        } else if self.current.len() < self.n && pending_tasks {
            cx.waker().clone().wake();
            Poll::Pending

        } else {
            Poll::Pending
        }
    }
}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> std::future::Future for FutureWindow<I, R, F> 
where
    R: Unpin,
    //<I as Iterator>::Item: core::fmt::Debug,
    //<R as Future>::Output: core::fmt::Debug,
{
    type Output = Vec<<R as Future>::Output>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
       Self::update(&mut self.as_mut(), cx)
    }
}

