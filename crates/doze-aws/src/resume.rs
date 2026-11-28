//! The services that carry on after a restart what was pending when the process stopped (a timer, a rotation, a poller), woken
//! beside the server once it listens.

use std::sync::Arc;

use doze_runtime::Tasks;

/// The services that need waking after a restart (what was pending when the process stopped carries on).
pub(crate) struct Resumable {
    pub(crate) cloudwatch: Option<Arc<aws_cloudwatch::Cloudwatch>>,
    pub(crate) eventbridge: Option<Arc<aws_eventbridge::Eventbridge>>,
    pub(crate) scheduler: Option<Arc<aws_scheduler::Scheduler>>,
    pub(crate) secrets: Option<Arc<aws_secrets::Secrets>>,
    pub(crate) dynamodb: Option<Arc<aws_dynamodb::Dynamodb>>,
    pub(crate) lambda: Option<Arc<aws_lambda::Lambda>>,
    pub(crate) states: Option<Arc<aws_sfn::Sfn>>,
    pub(crate) logs: Option<Arc<aws_logs::Logs>>,
}

impl Resumable {
    /// Wakes what was pending, in the services that are served, on a task beside the server: each opens its databases (on blocking
    /// threads), the services at once, so neither the ready line nor the first calls wait for them. Lambda is told where the gateway
    /// is first, before anything can invoke a function.
    pub(crate) fn resume(self, endpoint: &str, tasks: &Tasks) {
        if let Some(s) = &self.lambda {
            s.set_endpoint(endpoint);
        }
        tasks.spawn(async move {
            let Self {
                cloudwatch,
                eventbridge,
                scheduler,
                secrets,
                dynamodb,
                lambda,
                states,
                logs,
            } = self;
            tokio::join!(
                async {
                    if let Some(s) = cloudwatch {
                        s.resume().await;
                    }
                },
                async {
                    if let Some(s) = eventbridge {
                        s.resume().await;
                    }
                },
                async {
                    if let Some(s) = scheduler {
                        s.resume().await;
                    }
                },
                // Scheduled secret rotations: what came due while stopped starts now, and the next window is waited for.
                async {
                    if let Some(s) = secrets {
                        s.resume().await;
                    }
                },
                async {
                    if let Some(s) = dynamodb {
                        s.resume().await;
                    }
                },
                async {
                    if let Some(s) = lambda {
                        s.resume().await;
                    }
                },
                async {
                    if let Some(s) = states {
                        s.resume().await;
                    }
                },
                // Scheduled Logs Insights queries: the next run is waited for.
                async {
                    if let Some(s) = logs {
                        s.resume().await;
                    }
                },
            );
        });
    }
}
