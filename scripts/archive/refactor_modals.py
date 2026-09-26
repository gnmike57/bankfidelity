import re

files = [
    'src/app/gui.rs',
    'src/app/modals.rs',
]

replacements = [
    (r'self\.show_discard_draft_confirm = true;?', r'self.active_modal = ActiveModal::DiscardDraftConfirm;'),
    (r'self\.show_discard_draft_confirm = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_discard_draft_confirm\b', r'(self.active_modal == ActiveModal::DiscardDraftConfirm)'),
    
    (r'self\.show_workflow_hitl_modal = true;?', r'self.active_modal = ActiveModal::WorkflowHitl;'),
    (r'self\.show_workflow_hitl_modal = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_workflow_hitl_modal\b', r'(self.active_modal == ActiveModal::WorkflowHitl)'),
    
    (r'self\.show_settings_modal = true;?', r'self.active_modal = ActiveModal::Settings;'),
    (r'self\.show_settings_modal = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_settings_modal\b', r'(self.active_modal == ActiveModal::Settings)'),
    
    (r'self\.show_command_palette = true;?', r'self.active_modal = ActiveModal::CommandPalette;'),
    (r'self\.show_command_palette = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_command_palette\b', r'(self.active_modal == ActiveModal::CommandPalette)'),
    
    (r'self\.show_transfer_dialog = true;?', r'self.active_modal = ActiveModal::Transfer;'),
    (r'self\.show_transfer_dialog = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_transfer_dialog\b', r'(self.active_modal == ActiveModal::Transfer)'),
    
    (r'self\.show_feedback_modal = true;?', r'self.active_modal = ActiveModal::Feedback;'),
    (r'self\.show_feedback_modal = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_feedback_modal\b', r'(self.active_modal == ActiveModal::Feedback)'),
    
    (r'self\.show_date_adjust_dialog = true;?', r'self.active_modal = ActiveModal::DateAdjust;'),
    (r'self\.show_date_adjust_dialog = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_date_adjust_dialog\b', r'(self.active_modal == ActiveModal::DateAdjust)'),
    
    (r'self\.show_transfer_test_dialog = true;?', r'self.active_modal = ActiveModal::TransferTest;'),
    (r'self\.show_transfer_test_dialog = false;?', r'self.active_modal = ActiveModal::None;'),
    (r'self\.show_transfer_test_dialog\b', r'(self.active_modal == ActiveModal::TransferTest)'),
]

for file in files:
    with open(file, 'r') as f:
        content = f.read()
    
    for pat, rep in replacements:
        content = re.sub(pat, rep, content)
        
    with open(file, 'w') as f:
        f.write(content)

print("Refactored!")
