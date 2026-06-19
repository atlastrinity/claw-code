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
        next: &mut dyn FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome,
    ) -> ToolCallOutcome;
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
    
    // Pass self by value!
    pub fn process(
        mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        terminal: impl FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome + 'a,
    ) -> ToolCallOutcome {
        let mut next: Box<dyn FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> ToolCallOutcome + 'a> = Box::new(terminal);
        
        // Pop middlewares from the end, so they execute in order.
        while let Some(mut mw) = self.middlewares.pop() {
            let mut current_next = next;
            next = Box::new(move |ctx, prompter| {
                mw.process(ctx, prompter, &mut *current_next)
            });
        }
        
        next(ctx, prompter)
    }
}
fn main() {}
