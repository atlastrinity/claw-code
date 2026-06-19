pub struct ConversationMessage(pub String);
pub struct ToolCallRequest { pub tool_name: String }
pub trait PermissionPrompter {}
pub struct ToolCallContext { pub calls: Vec<ToolCallRequest> }
pub struct ToolCallOutcome { pub messages: Vec<ConversationMessage> }

pub trait TurnMiddleware: Send {
    fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        next: &mut dyn TurnMiddlewareHandler,
    ) -> ToolCallOutcome;
}

pub trait TurnMiddlewareHandler: Send {
    fn handle(&mut self, ctx: ToolCallContext, prompter: &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome;
}

impl<F> TurnMiddlewareHandler for F
where
    F: FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome + Send,
{
    fn handle(&mut self, ctx: ToolCallContext, prompter: &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome {
        self(ctx, prompter)
    }
}

pub struct MiddlewareChain<'a> {
    middlewares: Vec<Box<dyn TurnMiddleware + 'a>>,
}

impl<'a> MiddlewareChain<'a> {
    pub fn new() -> Self { Self { middlewares: Vec::new() } }
    pub fn with(mut self, middleware: impl TurnMiddleware + 'a) -> Self {
        self.middlewares.push(Box::new(middleware));
        self
    }
    
    pub fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        mut terminal: impl TurnMiddlewareHandler + 'a,
    ) -> ToolCallOutcome {
        let mut next: Box<dyn TurnMiddlewareHandler + 'a> = Box::new(terminal);
        
        for mw in self.middlewares.iter_mut().rev() {
            let mut current_next = next;
            next = Box::new(move |ctx: ToolCallContext, prompter: &mut Option<&mut dyn PermissionPrompter>| {
                mw.process(ctx, prompter, &mut *current_next)
            });
        }
        
        next.handle(ctx, prompter)
    }
}
fn main() {}
