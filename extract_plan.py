#!/usr/bin/env python3
"""Extract task list from Rollback_Netcode_Expanded_Master_Plan.xlsx"""

import zipfile
import xml.etree.ElementTree as ET
import json

xlsx_path = "Rollback_Netcode_Expanded_Master_Plan.xlsx"

def extract_text_from_cell(cell_elem):
    """Extract text from a cell (handles inline strings)"""
    # Try inline string
    is_elem = cell_elem.find('{http://schemas.openxmlformats.org/spreadsheetml/2006/main}is')
    if is_elem is not None:
        t_elem = is_elem.find('{http://schemas.openxmlformats.org/spreadsheetml/2006/main}t')
        if t_elem is not None:
            return t_elem.text
    
    # Try value
    v_elem = cell_elem.find('{http://schemas.openxmlformats.org/spreadsheetml/2006/main}v')
    if v_elem is not None:
        return v_elem.text
    
    return None

with zipfile.ZipFile(xlsx_path) as z:
    # Read Master Roadmap sheet (sheet1.xml)
    with z.open('xl/worksheets/sheet1.xml') as f:
        root = ET.parse(f).getroot()
        ns = '{http://schemas.openxmlformats.org/spreadsheetml/2006/main}'
        
        print("=== Master Roadmap Tasks ===\n")
        
        row_num = 0
        header_cols = None
        tasks = []
        
        for row in root.findall(f'{ns}sheetData/{ns}row'):
            row_num += 1
            
            cells = []
            for cell in row.findall(f'{ns}c'):
                val = extract_text_from_cell(cell)
                cells.append(val)
            
            # Skip if all empty
            if not any(cells):
                continue
            
            # First data row (row 1) is headers
            if row_num == 1:
                header_cols = cells
                print(f"Headers (first 8): {cells[:8]}\n")
                continue
            
            # Skip title/section rows
            if row_num <= 4:
                continue
            
            # Only process rows with data in first 3 columns
            if cells[0] or cells[1] or cells[2]:
                # Columns seem to be: A=Category, B=Item, C=Description, D=Priority, ... I=Hours?, ...
                task = {
                    'row': row_num,
                    'category': cells[0],
                    'item': cells[1],
                    'description': cells[2],
                    'phase': cells[3] if len(cells) > 3 else None,
                    'col_e': cells[4] if len(cells) > 4 else None,
                    'col_f': cells[5] if len(cells) > 5 else None,
                    'col_g': cells[6] if len(cells) > 6 else None,
                    'col_h': cells[7] if len(cells) > 7 else None,
                    'hours': cells[8] if len(cells) > 8 else None,  # Column I appears to be hours
                }
                if task['category'] or task['item']:
                    tasks.append(task)
                    print(f"Row {row_num}: {task['category']:20} | {task['item']:30} | H={task['hours']:>4} | {task['phase']}")
        
        print(f"\n\nTotal tasks: {len(tasks)}")
        
        # Write to JSON for later use
        with open('docs/evidence/plan_tasks.json', 'w') as f:
            json.dump(tasks, f, indent=2)

print("\nTasks saved to docs/evidence/plan_tasks.json")
