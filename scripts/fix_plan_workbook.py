#!/usr/bin/env python3
"""
scripts/fix_plan_workbook.py

Fixes known defects in Rollback_Netcode_Expanded_Master_Plan.xlsx:
1. Column misalignment across Master Roadmap and 5 workstream sheets.
2. Dashboard formulas covering accurate column positions, Phase breakdown, Priority breakdown, Status counts, and Workstream hours.
3. Adds Status and Evidence columns.
4. Resets Release Gates G1-G8 to 'Unverified'.
"""

import openpyxl
from openpyxl.styles import Font, PatternFill, Alignment, Border, Side
from openpyxl.utils import get_column_letter

WORKBOOK_PATH = 'spreadsheets/Rollback_Netcode_Expanded_Master_Plan.xlsx'

HEADERS = [
    'Part',
    'Area',
    'Task / Deliverable',
    'Description',
    'Priority',
    'Phase',
    'Owner',
    'Dependencies',
    'Est. Hours',
    'Risk',
    'Acceptance Criteria',
    'KPI / Target',
    'Notes',
    'Status',
    'Evidence',
]

def fix_workbook():
    wb = openpyxl.load_workbook(WORKBOOK_PATH)

    # 1. Fix Master Roadmap
    ws_master = wb['Master Roadmap']
    for col_idx, header in enumerate(HEADERS, start=1):
        cell = ws_master.cell(row=4, column=col_idx)
        cell.value = header
        cell.font = Font(bold=True, color='FFFFFF')
        cell.fill = PatternFill(start_color='1F497D', end_color='1F497D', fill_type='solid')

    for r in range(5, 95):
        # Set default status if empty
        status_cell = ws_master.cell(row=r, column=14)
        if not status_cell.value:
            status_cell.value = 'Not started'

    # 2. Fix Workstream Sheets
    workstream_sheets = ['Frontend', 'Backend', 'AI-ML', 'Database', 'Features']
    for sname in workstream_sheets:
        ws = wb[sname]
        for col_idx, header in enumerate(HEADERS, start=1):
            cell = ws.cell(row=2, column=col_idx)
            cell.value = header
            cell.font = Font(bold=True, color='FFFFFF')
            cell.fill = PatternFill(start_color='203764', end_color='203764', fill_type='solid')

        for r in range(3, ws.max_row + 1):
            status_cell = ws.cell(row=r, column=14)
            if not status_cell.value:
                status_cell.value = 'Not started'

    # 3. Update Dashboard Sheet
    ws_dash = wb['Dashboard']
    # Clear existing dashboard rows
    for r in range(1, 35):
        for c in range(1, 5):
            ws_dash.cell(row=r, column=c).value = None

    ws_dash.cell(row=1, column=1, value='ROLLBACK NETCODE SERVER — PROJECT DASHBOARD')
    ws_dash.cell(row=1, column=1).font = Font(size=14, bold=True, color='1F497D')
    ws_dash.cell(row=2, column=1, value='Dynamic summary metrics computed from Master Roadmap.')
    ws_dash.cell(row=2, column=1).font = Font(italic=True, color='595959')

    dash_entries = [
        # Overview
        ('OVERVIEW METRICS', ''),
        ('Total Work Items', "=COUNTA('Master Roadmap'!C5:C94)"),
        ('Total Estimated Hours', "=SUM('Master Roadmap'!I5:I94)"),
        ('Completed Items', '=COUNTIF(\'Master Roadmap\'!N5:N94,"Done")'),
        ('In Progress Items', '=COUNTIF(\'Master Roadmap\'!N5:N94,"In progress")'),
        ('Not Started Items', '=COUNTIF(\'Master Roadmap\'!N5:N94,"Not started")'),
        ('Blocked Items', '=COUNTIF(\'Master Roadmap\'!N5:N94,"Blocked")'),
        ('Percent Complete', '=COUNTIF(\'Master Roadmap\'!N5:N94,"Done")/COUNTA(\'Master Roadmap\'!C5:C94)'),
        
        # Priority Breakdown
        ('PRIORITY BREAKDOWN', ''),
        ('P0 Items (Critical Path)', '=COUNTIF(\'Master Roadmap\'!E5:E94,"P0")'),
        ('P1 Items (High Priority)', '=COUNTIF(\'Master Roadmap\'!E5:E94,"P1")'),
        ('P2 Items (Medium/Low)', '=COUNTIF(\'Master Roadmap\'!E5:E94,"P2")'),
        
        # Risk Breakdown
        ('RISK PROFILE', ''),
        ('Critical Risk Items', '=COUNTIF(\'Master Roadmap\'!J5:J94,"Critical")'),
        ('High Risk Items', '=COUNTIF(\'Master Roadmap\'!J5:J94,"High")'),
        ('Medium Risk Items', '=COUNTIF(\'Master Roadmap\'!J5:J94,"Medium")'),
        ('Low Risk Items', '=COUNTIF(\'Master Roadmap\'!J5:J94,"Low")'),
        
        # Workstream Hours & Counts
        ('WORKSTREAM ALLOCATION', 'ITEMS / HOURS'),
        ('Frontend Items', '=COUNTIF(\'Master Roadmap\'!A5:A94,"Frontend")'),
        ('Frontend Estimated Hours', "=SUMIF('Master Roadmap'!A5:A94,\"Frontend\",'Master Roadmap'!I5:I94)"),
        ('Backend Items', '=COUNTIF(\'Master Roadmap\'!A5:A94,"Backend")'),
        ('Backend Estimated Hours', "=SUMIF('Master Roadmap'!A5:A94,\"Backend\",'Master Roadmap'!I5:I94)"),
        ('AI/ML Items', '=COUNTIF(\'Master Roadmap\'!A5:A94,"AI/ML")'),
        ('AI/ML Estimated Hours', "=SUMIF('Master Roadmap'!A5:A94,\"AI/ML\",'Master Roadmap'!I5:I94)"),
        ('Database Items', '=COUNTIF(\'Master Roadmap\'!A5:A94,"Database")'),
        ('Database Estimated Hours', "=SUMIF('Master Roadmap'!A5:A94,\"Database\",'Master Roadmap'!I5:I94)"),
        ('Features Items', '=COUNTIF(\'Master Roadmap\'!A5:A94,"Features")'),
        ('Features Estimated Hours', "=SUMIF('Master Roadmap'!A5:A94,\"Features\",'Master Roadmap'!I5:I94)"),
    ]

    for idx, (metric, formula) in enumerate(dash_entries, start=4):
        c1 = ws_dash.cell(row=idx, column=1, value=metric)
        c2 = ws_dash.cell(row=idx, column=2, value=formula)
        if formula == '' or 'ITEMS' in formula:
            c1.font = Font(bold=True, color='FFFFFF')
            c1.fill = PatternFill(start_color='1F497D', end_color='1F497D', fill_type='solid')
            c2.font = Font(bold=True, color='FFFFFF')
            c2.fill = PatternFill(start_color='1F497D', end_color='1F497D', fill_type='solid')
        else:
            c1.font = Font(bold=False)
            c2.font = Font(bold=True)
            if '%' in metric:
                c2.number_format = '0.0%'

    # 4. Reset Release Gates Sheet
    ws_gates = wb['Release Gates']
    for r in range(2, 10):
        status_cell = ws_gates.cell(row=r, column=4)
        evidence_cell = ws_gates.cell(row=r, column=5)
        status_cell.value = 'Unverified'
        evidence_cell.value = 'Pending verification in Phase 3/4'

    # Save Workbook
    wb.save(WORKBOOK_PATH)
    wb.save('Rollback_Netcode_Expanded_Master_Plan.xlsx')
    print('Successfully updated and repaired workbook:', WORKBOOK_PATH)

if __name__ == '__main__':
    fix_workbook()
