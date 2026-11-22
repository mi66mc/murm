use super::Action;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Screen{Profiles,ProfileForm,Unlock,Relays,Articles,Article,History,Drafts,Outbox,Confirm}
pub struct AppState{
    pub screen:Screen,pub running:bool,pub selected:usize,pub scroll:usize,pub help:bool,
    pub generation:u64,pub status:String,
    previous:Vec<Screen>,
}
impl Default for AppState{
    fn default()->Self{Self{screen:Screen::Profiles,running:true,selected:0,scroll:0,help:false,generation:0,status:String::new(),previous:vec![]}}
}
impl AppState{
    pub fn reduce(&mut self,action:Action){
        match action{
            Action::Navigate(screen)=>{
                if self.screen!=screen{self.previous.push(self.screen);self.screen=screen;}
                self.selected=0;self.scroll=0;
            }
            Action::Back=>{self.screen=self.previous.pop().unwrap_or(Screen::Profiles);self.selected=0;self.scroll=0;}
            Action::Next=>{self.selected=self.selected.saturating_add(1);self.scroll=self.scroll.saturating_add(1);}
            Action::Previous=>{self.selected=self.selected.saturating_sub(1);self.scroll=self.scroll.saturating_sub(1);}
            Action::PageDown=>{self.scroll=self.scroll.saturating_add(10);}
            Action::PageUp=>{self.scroll=self.scroll.saturating_sub(10);}
            Action::Help=>self.help=!self.help,
            Action::Quit=>self.running=false,
        }
    }
}
