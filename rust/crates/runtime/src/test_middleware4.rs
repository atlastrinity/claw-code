pub struct ConversationMessage(pub String);
pub struct ToolCallRequest { pub tool_name: String }
pub trait PermissionPrompter {}
pub struct ToolCallContext { pub calls: Vec<ToolCallRequest> }

pub trait TurnMiddleware<'a>: Send {
    fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        next: &mut dyn FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> Vec<ConversationMessage>,
    ) -> Vec<ConversationMessage>;
}

pub struct MiddlewareChain<'a> {
    middlewares: Vec<Box<dyn TurnMiddleware<'a> + 'a>>,
}

impl<'a> MiddlewareChain<'a> {
    pub fn new() -> Self { Self { middlewares: Vec::new() } }
    pub fn with(mut self, middleware: impl TurnMiddleware<'a> + 'a) -> Self {
        self.middlewares.push(Box::new(middleware));
        self
    }
    
    pub fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut dyn PermissionPrompter>,
        terminal: impl FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> Vec<ConversationMessage> + 'a,
    ) -> Vec<ConversationMessage> {
        let mut next: Box<dyn FnMut(ToolCallContext, &mut Option<&mut dyn PermissionPrompter>) -> Vec<ConversationMessage> + 'a> = Box::new(terminal);
        
        for mw in self.middlewares.iter_mut().rev() {
            let mut current_next = next;
            next = Box::new(move |ctx, prompter| mw.process(ctx, prompter, &mut *current_next));
        }
        
        next(ctx, prompter)
    }
}
fn main() {}
